/// Binance feeder with integrated OrderBookBuilder
///
/// This extends the existing Binance feeder to build local orderbooks
/// from the WebSocket depth updates

use async_trait::async_trait;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tracing::{debug, info, warn, error};
use tokio::sync::mpsc;
use std::sync::Arc;
use std::collections::HashMap;
use matching_engine::orderbook_builder::{OrderBookBuilder, OrderBookConfig};
use market_types::{OrderBook, OrderBookUpdate, Exchange, PriceLevel};

use super::websocket_config::connect_with_large_buffer;
use super::binance::BinanceExchange;
use crate::core::{
    Feeder, TRADES, ORDERBOOKS, COMPARE_NOTIFY, TradeData, OrderBookData,
    SymbolMapper, get_shutdown_receiver, CONNECTION_STATS,
    MultiPortUdpSender, get_multi_port_sender,
    parse_to_scaled_or_default,
};
use crate::error::{Result, Error};
use crate::load_config::ExchangeConfig;

/// Extended Binance exchange with OrderBookBuilder integration
pub struct BinanceWithOrderBook {
    // Base Binance feeder
    inner: BinanceExchange,

    // OrderBook builders per symbol
    orderbook_builders: HashMap<String, OrderBookBuilderHandle>,

    // Channel to receive built orderbooks
    orderbook_receiver: Option<mpsc::Receiver<(String, OrderBook)>>,
    orderbook_sender: mpsc::Sender<(String, OrderBook)>,
}

/// Handle for managing an OrderBookBuilder instance
struct OrderBookBuilderHandle {
    update_sender: mpsc::Sender<OrderBookUpdate>,
    snapshot_receiver: mpsc::Receiver<OrderBook>,
    builder_task: Option<tokio::task::JoinHandle<()>>,
}

impl BinanceWithOrderBook {
    pub fn new(
        config: ExchangeConfig,
        symbol_mapper: Arc<SymbolMapper>,
    ) -> Self {
        let inner = BinanceExchange::new(config.clone(), symbol_mapper.clone());
        let (orderbook_sender, orderbook_receiver) = mpsc::channel(100);

        Self {
            inner,
            orderbook_builders: HashMap::new(),
            orderbook_receiver: Some(orderbook_receiver),
            orderbook_sender,
        }
    }

    /// Create OrderBookBuilder for a symbol if it doesn't exist
    fn ensure_orderbook_builder(&mut self, symbol: &str) -> mpsc::Sender<OrderBookUpdate> {
        if let Some(handle) = self.orderbook_builders.get(symbol) {
            return handle.update_sender.clone();
        }

        // Create new builder
        let (update_tx, update_rx) = mpsc::channel(1000);
        let (snapshot_tx, snapshot_rx) = mpsc::channel(10);

        let config = OrderBookConfig {
            max_levels: 100,
            buffer_size: 1000,
            checksum_interval: Some(100),
            max_resync_attempts: 5,
            resync_delay_ms: 1000,
            snapshot_url: Some(format!(
                "https://api.binance.com/api/v3/depth?symbol={}&limit=1000",
                symbol.to_uppercase()
            )),
            enable_persistence: false, // Disable for now
            persistence_interval: None,
        };

        let builder = OrderBookBuilder::with_config(
            symbol.to_string(),
            Exchange::Binance,
            update_rx,
            snapshot_tx,
            config,
        );

        // Forward snapshots to our collector
        let symbol_clone = symbol.to_string();
        let orderbook_sender = self.orderbook_sender.clone();
        let snapshot_forwarder = tokio::spawn(async move {
            let mut rx = snapshot_rx;
            while let Some(snapshot) = rx.recv().await {
                let _ = orderbook_sender.send((symbol_clone.clone(), snapshot)).await;
            }
        });

        // Start the builder
        let symbol_for_log = symbol.to_string();
        let builder_task = tokio::spawn(async move {
            if let Err(e) = builder.run().await {
                error!("OrderBookBuilder error for {}: {}", symbol_for_log, e);
            }
        });

        let handle = OrderBookBuilderHandle {
            update_sender: update_tx.clone(),
            snapshot_receiver: snapshot_forwarder.into(),
            builder_task: Some(builder_task),
        };

        self.orderbook_builders.insert(symbol.to_string(), handle);
        info!("Created OrderBookBuilder for {}", symbol);

        update_tx
    }

    /// Process depth update message and send to OrderBookBuilder
    fn process_depth_update(&mut self, data: &Value, symbol: &str, stream_name: &str) {
        // Determine if this is a snapshot or update
        let is_snapshot = stream_name.contains("@depth") && !data.get("U").is_some();

        // Parse bids and asks
        let bids = data.get("b")
            .or_else(|| data.get("bids"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let price = item[0].as_str()?.parse::<f64>().ok()?;
                        let qty = item[1].as_str()?.parse::<f64>().ok()?;
                        Some(PriceLevel { price, quantity: qty })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let asks = data.get("a")
            .or_else(|| data.get("asks"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let price = item[0].as_str()?.parse::<f64>().ok()?;
                        let qty = item[1].as_str()?.parse::<f64>().ok()?;
                        Some(PriceLevel { price, quantity: qty })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Create OrderBookUpdate
        let update = OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: symbol.to_uppercase(),
            timestamp: data.get("E")
                .and_then(|v| v.as_i64())
                .unwrap_or_else(|| chrono::Utc::now().timestamp_micros()),
            bids,
            asks,
            is_snapshot,
            update_id: data.get("u")
                .or_else(|| data.get("lastUpdateId"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            first_update_id: data.get("U")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            prev_update_id: data.get("pu")
                .and_then(|v| v.as_u64()),
        };

        // Send to OrderBookBuilder
        let update_sender = self.ensure_orderbook_builder(symbol);
        let _ = update_sender.try_send(update);
    }
}

#[async_trait]
impl Feeder for BinanceWithOrderBook {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn connect(&mut self) -> Result<()> {
        self.inner.connect().await
    }

    async fn subscribe(&mut self) -> Result<()> {
        self.inner.subscribe().await
    }

    async fn start(&mut self) -> Result<()> {
        // Start orderbook consumer task
        if let Some(mut orderbook_rx) = self.orderbook_receiver.take() {
            tokio::spawn(async move {
                while let Some((symbol, orderbook)) = orderbook_rx.recv().await {
                    info!(
                        "📚 Built OrderBook for {}: {} bids, {} asks, seq: {}",
                        symbol,
                        orderbook.bids.len(),
                        orderbook.asks.len(),
                        orderbook.last_update_id
                    );

                    // Log best bid/ask
                    if let (Some(best_bid), Some(best_ask)) =
                        (orderbook.bids.first(), orderbook.asks.first()) {
                        info!(
                            "  {} Best: Bid {:.2} @ {:.4}, Ask {:.2} @ {:.4}, Spread: {:.2}",
                            symbol,
                            best_bid.price, best_bid.quantity,
                            best_ask.price, best_ask.quantity,
                            best_ask.price - best_bid.price
                        );
                    }

                    // Here you can:
                    // 1. Send the built orderbook via UDP
                    // 2. Store it in a shared state
                    // 3. Use it for trading decisions
                    // 4. Forward to other systems
                }
            });
        }

        // Start the base feeder
        self.inner.start().await
    }
}

/// Parse Binance message and route depth updates to OrderBookBuilder
pub fn process_message_with_builder(
    text: &str,
    symbol_mapper: Arc<SymbolMapper>,
    asset_type: &str,
    orderbook_sender: &mut mpsc::Sender<OrderBookUpdate>,
) {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("Failed to parse message: {}", e);
            return;
        }
    };

    // Extract stream name
    let stream_name = value.get("stream").and_then(|v| v.as_str()).unwrap_or("");

    // Get actual data
    let actual_data = if value.get("stream").is_some() && value.get("data").is_some() {
        &value["data"]
    } else {
        &value
    };

    // Process depth updates
    if stream_name.contains("@depth") {
        // Extract symbol from stream name
        let original_symbol = stream_name.split('@').next().unwrap_or("").to_uppercase();

        if !original_symbol.is_empty() {
            if let Some(common_symbol) = symbol_mapper.map("Binance", &original_symbol) {
                // Parse and send to OrderBookBuilder
                let is_snapshot = !actual_data.get("U").is_some();

                let bids = actual_data.get("b")
                    .or_else(|| actual_data.get("bids"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let price = item[0].as_str()?.parse::<f64>().ok()?;
                                let qty = item[1].as_str()?.parse::<f64>().ok()?;
                                Some(PriceLevel { price, quantity: qty })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let asks = actual_data.get("a")
                    .or_else(|| actual_data.get("asks"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let price = item[0].as_str()?.parse::<f64>().ok()?;
                                let qty = item[1].as_str()?.parse::<f64>().ok()?;
                                Some(PriceLevel { price, quantity: qty })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let update = OrderBookUpdate {
                    exchange: Exchange::Binance,
                    symbol: common_symbol,
                    timestamp: actual_data.get("E")
                        .and_then(|v| v.as_i64())
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_micros()),
                    bids,
                    asks,
                    is_snapshot,
                    update_id: actual_data.get("u")
                        .or_else(|| actual_data.get("lastUpdateId"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    first_update_id: actual_data.get("U")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    prev_update_id: actual_data.get("pu")
                        .and_then(|v| v.as_u64()),
                };

                let _ = orderbook_sender.try_send(update);
            }
        }
    }
}