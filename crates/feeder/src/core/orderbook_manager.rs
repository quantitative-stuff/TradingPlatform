/// OrderBook Manager for integration with feeders
///
/// Manages OrderBookBuilder instances for multiple symbols and exchanges
/// Designed to be integrated into existing exchange feeders

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use matching_engine::orderbook_builder::OrderBookBuilder;
use market_types::{OrderBook, OrderBookUpdate, Exchange};
use tracing::{info, warn, error, debug};
use anyhow::Result;

/// Global OrderBook manager instance
static ORDERBOOK_MANAGER: once_cell::sync::OnceCell<Arc<OrderBookManager>> = once_cell::sync::OnceCell::new();

/// Initialize global OrderBook manager
pub fn init_orderbook_manager() -> Arc<OrderBookManager> {
    ORDERBOOK_MANAGER.get_or_init(|| {
        Arc::new(OrderBookManager::new())
    }).clone()
}

/// Get global OrderBook manager
pub fn get_orderbook_manager() -> Option<Arc<OrderBookManager>> {
    ORDERBOOK_MANAGER.get().cloned()
}

/// Manages OrderBookBuilder instances for multiple symbols
pub struct OrderBookManager {
    /// Builders indexed by (exchange, symbol)
    builders: Arc<RwLock<HashMap<(Exchange, String), OrderBookHandle>>>,

    /// Channel to receive built orderbooks
    orderbook_receiver: Arc<RwLock<Option<mpsc::Receiver<BuiltOrderBook>>>>,
    orderbook_sender: mpsc::Sender<BuiltOrderBook>,

    /// UDP sender for built orderbooks (optional)
    udp_sender: Arc<RwLock<Option<Arc<dyn OrderBookSender + Send + Sync>>>>,
}

/// Handle for a single OrderBookBuilder
struct OrderBookHandle {
    update_sender: mpsc::Sender<OrderBookUpdate>,
    builder_task: tokio::task::JoinHandle<()>,
}

/// Built orderbook with metadata
#[derive(Debug, Clone)]
pub struct BuiltOrderBook {
    pub exchange: Exchange,
    pub symbol: String,
    pub orderbook: OrderBook,
    pub timestamp: i64,
}

/// Trait for sending built orderbooks
pub trait OrderBookSender {
    fn send_orderbook(&self, orderbook: &BuiltOrderBook) -> Result<()>;
}

impl OrderBookManager {
    /// Create new OrderBook manager
    pub fn new() -> Self {
        let (orderbook_sender, orderbook_receiver) = mpsc::channel(1000);

        Self {
            builders: Arc::new(RwLock::new(HashMap::new())),
            orderbook_receiver: Arc::new(RwLock::new(Some(orderbook_receiver))),
            orderbook_sender,
            udp_sender: Arc::new(RwLock::new(None)),
        }
    }

    /// Set UDP sender for built orderbooks
    pub async fn set_udp_sender(&self, sender: Arc<dyn OrderBookSender + Send + Sync>) {
        *self.udp_sender.write().await = Some(sender);
        info!("OrderBook manager UDP sender configured");
    }

    /// Start the orderbook consumer task
    pub async fn start_consumer(&self) {
        let mut receiver = {
            let mut rx_lock = self.orderbook_receiver.write().await;
            match rx_lock.take() {
                Some(rx) => rx,
                None => {
                    warn!("OrderBook consumer already started");
                    return;
                }
            }
        };

        let udp_sender = self.udp_sender.clone();

        tokio::spawn(async move {
            info!("OrderBook consumer started");

            while let Some(built) = receiver.recv().await {
                debug!(
                    "📚 Built OrderBook: {} {} - {} bids, {} asks, seq: {}",
                    built.exchange.to_string(),
                    built.symbol,
                    built.orderbook.bids.len(),
                    built.orderbook.asks.len(),
                    built.orderbook.last_update_id
                );

                // Send via UDP if configured
                if let Some(sender) = udp_sender.read().await.as_ref() {
                    if let Err(e) = sender.send_orderbook(&built) {
                        warn!("Failed to send orderbook via UDP: {}", e);
                    }
                }

                // Log best bid/ask for monitoring
                if let (Some(best_bid), Some(best_ask)) =
                    (built.orderbook.bids.first(), built.orderbook.asks.first()) {
                    debug!(
                        "  {} Best: Bid {:.2} @ {:.4}, Ask {:.2} @ {:.4}, Spread: {:.2}",
                        built.symbol,
                        best_bid.price, best_bid.quantity,
                        best_ask.price, best_ask.quantity,
                        best_ask.price - best_bid.price
                    );
                }
            }

            warn!("OrderBook consumer ended");
        });
    }

    /// Get or create OrderBookBuilder for a symbol
    pub async fn get_or_create_builder(
        &self,
        exchange: Exchange,
        symbol: &str
    ) -> mpsc::Sender<OrderBookUpdate> {
        let key = (exchange, symbol.to_string());

        // Check if builder exists
        {
            let builders = self.builders.read().await;
            if let Some(handle) = builders.get(&key) {
                return handle.update_sender.clone();
            }
        }

        // Create new builder
        let (update_tx, update_rx) = mpsc::channel(1000);
        let (snapshot_tx, mut snapshot_rx) = mpsc::channel(10);

        // Create builder
        let builder = OrderBookBuilder::new(
            symbol.to_string(),
            exchange,
            update_rx,
            snapshot_tx,
        );

        // Forward snapshots to our collector
        let symbol_clone = symbol.to_string();
        let exchange_clone = exchange;
        let orderbook_sender = self.orderbook_sender.clone();

        tokio::spawn(async move {
            while let Some(orderbook) = snapshot_rx.recv().await {
                let built = BuiltOrderBook {
                    exchange: exchange_clone,
                    symbol: symbol_clone.clone(),
                    orderbook,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };

                if let Err(e) = orderbook_sender.send(built).await {
                    error!("Failed to forward built orderbook: {}", e);
                    break;
                }
            }
        });

        // Start the builder
        let symbol_for_log = symbol.to_string();
        let exchange_for_log = exchange;
        let builder_task = tokio::spawn(async move {
            info!("Starting OrderBookBuilder for {} on {:?}", symbol_for_log, exchange_for_log);
            if let Err(e) = builder.run().await {
                error!("OrderBookBuilder error for {} on {:?}: {}",
                    symbol_for_log, exchange_for_log, e);
            }
        });

        // Store handle
        let handle = OrderBookHandle {
            update_sender: update_tx.clone(),
            builder_task,
        };

        let mut builders = self.builders.write().await;
        builders.insert(key, handle);

        info!("Created OrderBookBuilder for {} on {:?}", symbol, exchange);

        update_tx
    }

    /// Send update to OrderBookBuilder
    pub async fn send_update(&self, update: OrderBookUpdate) -> Result<()> {
        let sender = self.get_or_create_builder(update.exchange, &update.symbol).await;

        sender.send(update).await
            .map_err(|e| anyhow::anyhow!("Failed to send update: {}", e))
    }

    /// Try to send update (non-blocking)
    pub async fn try_send_update(&self, update: OrderBookUpdate) -> Result<()> {
        let sender = self.get_or_create_builder(update.exchange, &update.symbol).await;

        sender.try_send(update)
            .map_err(|e| anyhow::anyhow!("Failed to send update: {}", e))
    }

    /// Get current orderbook snapshot for a symbol
    pub async fn get_orderbook(&self, exchange: Exchange, symbol: &str) -> Option<OrderBook> {
        // This would require adding a query mechanism to OrderBookBuilder
        // For now, return None
        None
    }

    /// Shutdown all builders
    pub async fn shutdown(&self) {
        let builders = self.builders.write().await;
        for (_, handle) in builders.iter() {
            handle.builder_task.abort();
        }
        info!("OrderBook manager shutdown complete");
    }
}

/// UDP sender implementation for built orderbooks
pub struct UdpOrderBookSender {
    sender: Arc<crate::core::BinaryUdpSender>,
}

impl UdpOrderBookSender {
    pub fn new(sender: Arc<crate::core::BinaryUdpSender>) -> Self {
        Self { sender }
    }
}

impl OrderBookSender for UdpOrderBookSender {
    fn send_orderbook(&self, built: &BuiltOrderBook) -> Result<()> {
        // Convert to OrderBookData format for UDP
        let orderbook_data = crate::core::OrderBookData {
            exchange: built.exchange.to_string(),
            symbol: built.symbol.clone(),
            asset_type: "spot".to_string(), // Would need to determine this
            bids: built.orderbook.bids.iter()
                .map(|level| (
                    (level.price * 100000000.0) as i64,  // Convert to scaled i64
                    (level.quantity * 100000000.0) as i64
                ))
                .collect(),
            asks: built.orderbook.asks.iter()
                .map(|level| (
                    (level.price * 100000000.0) as i64,
                    (level.quantity * 100000000.0) as i64
                ))
                .collect(),
            price_precision: 8,
            quantity_precision: 8,
            timestamp: built.timestamp as u64,
            timestamp_unit: crate::load_config::TimestampUnit::Milliseconds,
        };

        self.sender.send_orderbook_data(orderbook_data)
            .map_err(|e| anyhow::anyhow!("UDP send failed: {}", e))
    }
}