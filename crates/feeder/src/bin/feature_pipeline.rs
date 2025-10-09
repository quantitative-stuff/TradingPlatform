/// Feature Pipeline Binary
///
/// Receives market data from multicast UDP → Matching Engine → Feature Engineering
///
/// Data flow:
///   1. MultiPortUdpReceiver (20 parallel streams) receives binary packets
///   2. Parse binary packets to MarketTypes (OrderBookUpdate, Trade)
///   3. Process through MatchingEngine (L2 → L3 events)
///   4. Calculate features via FeaturePipeline
///   5. Output features to console/file/database
///
/// Usage:
///   cargo run --bin feature_pipeline --release

use feeder::core::multi_port_udp_receiver::{MultiPortUdpReceiver, DataType};
use feeder::core::binary_udp_packet::{PacketHeader, OrderBookItem, TradeItem};
use feature_engineering::pipeline::FeaturePipeline;
use market_types::{OrderBookUpdate, Trade, PriceLevel, Exchange, Side};
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{info, error, warn};
use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::RwLock;

const LOOKBACK_MS: i64 = 1000; // 1 second lookback for features

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .init();

    info!("╔════════════════════════════════════════════════════════╗");
    info!("║       Feature Pipeline - Real-time Processing         ║");
    info!("╠════════════════════════════════════════════════════════╣");
    info!("║  UDP Receiver → Matching Engine → Features            ║");
    info!("║                                                        ║");
    info!("║  Receiving from 20 multicast streams:                 ║");
    info!("║    Binance, Bybit, OKX, Deribit, Upbit,              ║");
    info!("║    Coinbase, Bithumb                                   ║");
    info!("╚════════════════════════════════════════════════════════╝");

    // Create feature pipeline (includes matching engine)
    let pipeline = Arc::new(FeaturePipeline::new(LOOKBACK_MS));
    info!("✅ Feature pipeline initialized (lookback: {}ms)", LOOKBACK_MS);

    // Create channels for orderbook updates and trades
    let (orderbook_tx, mut orderbook_rx) = mpsc::channel::<OrderBookUpdate>(10000);
    let (trade_tx, mut trade_rx) = mpsc::channel::<Trade>(10000);

    // Spawn orderbook processing task
    let pipeline_clone = pipeline.clone();
    tokio::spawn(async move {
        let mut feature_count = 0u64;
        let mut last_log_time = std::time::Instant::now();

        while let Some(update) = orderbook_rx.recv().await {
            match pipeline_clone.process_order_book_update(update.clone()) {
                Ok(Some(features)) => {
                    feature_count += 1;

                    // Log every 10 seconds
                    if last_log_time.elapsed().as_secs() >= 10 {
                        info!(
                            "[{}:{}] Features: OFI={:.4}, Spread={:.4}, WAP_0ms={:.2}",
                            features.exchange,
                            features.symbol,
                            features.order_flow_imbalance.unwrap_or(0.0),
                            features.spread.unwrap_or(0.0),
                            features.wap_0ms.unwrap_or(0.0),
                        );
                        info!("Total features calculated: {}", feature_count);
                        last_log_time = std::time::Instant::now();
                    }
                }
                Ok(None) => {
                    // No features yet (not enough data)
                }
                Err(e) => {
                    error!("Error processing orderbook update: {}", e);
                }
            }
        }
    });

    // Spawn trade processing task
    let pipeline_clone = pipeline.clone();
    tokio::spawn(async move {
        while let Some(trade) = trade_rx.recv().await {
            if let Err(e) = pipeline_clone.process_trade(trade) {
                error!("Error processing trade: {}", e);
            }
        }
    });

    // Packet aggregation state
    let aggregator = Arc::new(RwLock::new(OrderBookAggregator::new()));

    // Create receiver with packet handler
    let orderbook_tx_clone = orderbook_tx.clone();
    let trade_tx_clone = trade_tx.clone();
    let aggregator_clone = aggregator.clone();

    let receiver = MultiPortUdpReceiver::new(move |data, exchange_name, receiver_id, data_type| {
        let orderbook_tx = orderbook_tx_clone.clone();
        let trade_tx = trade_tx_clone.clone();
        let aggregator = aggregator_clone.clone();

        async move {
            // Parse binary packet
            if data.len() < 58 {
                warn!("Received packet too small from {}: {} bytes", receiver_id, data.len());
                return;
            }

            // Parse header (58 bytes)
            let header = parse_packet_header(&data);
            let symbol = parse_string(&header.symbol);
            let exchange = parse_exchange(&exchange_name);

            match data_type {
                DataType::Trade => {
                    if let Some(trade_item) = parse_trade_packet(&data, &header) {
                        // Copy trade_id to avoid packed field reference
                        let trade_id_val = trade_item.trade_id;

                        let trade = Trade {
                            exchange,
                            symbol: symbol.clone(),
                            timestamp: (header.exchange_timestamp / 1000) as i64, // ns to μs
                            price: trade_item.price as f64 / 1e8,
                            quantity: trade_item.quantity as f64 / 1e8,
                            side: if trade_item.side == 1 { Side::Buy } else { Side::Sell },
                            trade_id: trade_id_val.to_string(),
                        };

                        if let Err(e) = trade_tx.send(trade).await {
                            error!("Failed to send trade to pipeline: {}", e);
                        }
                    }
                }
                DataType::OrderBook => {
                    if let Some(levels) = parse_orderbook_levels(&data, &header) {
                        let is_bid = header.is_bid();
                        let is_last = header.is_last();
                        let key = (exchange, symbol.clone());
                        let timestamp = (header.exchange_timestamp / 1000) as i64; // ns to μs

                        // Aggregate orderbook packets (bids and asks come separately)
                        let update_opt = {
                            let mut agg = aggregator.write();

                            if is_bid {
                                agg.add_bids(key.clone(), levels);
                            } else {
                                agg.add_asks(key.clone(), levels);
                            }

                            // If this is the last packet, get complete orderbook
                            if is_last {
                                agg.take_complete(&key).map(|(bids, asks)| OrderBookUpdate {
                                    exchange,
                                    symbol: symbol.clone(),
                                    timestamp,
                                    bids,
                                    asks,
                                    is_snapshot: true,
                                })
                            } else {
                                None
                            }
                        }; // Drop lock here

                        // Send outside of lock
                        if let Some(update) = update_opt {
                            if let Err(e) = orderbook_tx.send(update).await {
                                error!("Failed to send orderbook to pipeline: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }).await?;

    info!("✅ Multi-Port Receiver started successfully");
    info!("✅ Feature pipeline processing started");
    info!("Listening on 20 multicast addresses...");
    info!("Press Ctrl+C to stop");

    // Wait for Ctrl+C
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Shutting down...");
        }
    }

    // Print final stats
    receiver.print_stats();
    receiver.abort_all();

    info!("Feature pipeline stopped");
    Ok(())
}

/// Parse packet header from raw bytes
fn parse_packet_header(data: &[u8]) -> PacketHeader {
    unsafe {
        let mut header: PacketHeader = std::ptr::read(data.as_ptr() as *const PacketHeader);
        header.from_network_order();
        header
    }
}

/// Parse null-terminated string from byte array
fn parse_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

/// Parse exchange name to Exchange enum
fn parse_exchange(name: &str) -> Exchange {
    match name.to_lowercase().as_str() {
        "binance" => Exchange::Binance,
        "bybit" => Exchange::Bybit,
        "okx" => Exchange::OKX,
        "deribit" => Exchange::Deribit,
        "upbit" => Exchange::Upbit,
        "coinbase" => Exchange::Coinbase,
        "bithumb" => Exchange::Bithumb,
        _ => {
            warn!("Unknown exchange: {}, defaulting to Binance", name);
            Exchange::Binance
        }
    }
}

/// Parse trade packet payload
fn parse_trade_packet(data: &[u8], header: &PacketHeader) -> Option<TradeItem> {
    let item_count = header.item_count() as usize;
    if item_count == 0 || data.len() < 58 + 32 {
        return None;
    }

    unsafe {
        let mut trade: TradeItem = std::ptr::read(data[58..].as_ptr() as *const TradeItem);
        // Convert from network byte order
        trade.trade_id = u64::from_be(trade.trade_id);
        trade.price = i64::from_be(trade.price);
        trade.quantity = i64::from_be(trade.quantity);
        Some(trade)
    }
}

/// Parse orderbook packet payload
fn parse_orderbook_levels(data: &[u8], header: &PacketHeader) -> Option<Vec<PriceLevel>> {
    let item_count = header.item_count() as usize;
    if item_count == 0 || data.len() < 58 + (item_count * 16) {
        return None;
    }

    let mut levels = Vec::with_capacity(item_count);
    let payload = &data[58..];

    for i in 0..item_count {
        let offset = i * 16;
        if offset + 16 > payload.len() {
            break;
        }

        unsafe {
            let mut item: OrderBookItem = std::ptr::read(payload[offset..].as_ptr() as *const OrderBookItem);
            // Convert from network byte order
            item.price = i64::from_be(item.price);
            item.quantity = i64::from_be(item.quantity);

            levels.push(PriceLevel {
                price: item.price as f64 / 1e8,
                quantity: item.quantity as f64 / 1e8,
            });
        }
    }

    Some(levels)
}

/// Aggregates orderbook packets (bids/asks come separately)
struct OrderBookAggregator {
    partial_books: HashMap<(Exchange, String), PartialOrderBook>,
}

struct PartialOrderBook {
    bids: Option<Vec<PriceLevel>>,
    asks: Option<Vec<PriceLevel>>,
}

impl OrderBookAggregator {
    fn new() -> Self {
        Self {
            partial_books: HashMap::new(),
        }
    }

    fn add_bids(&mut self, key: (Exchange, String), bids: Vec<PriceLevel>) {
        self.partial_books
            .entry(key)
            .or_insert_with(|| PartialOrderBook { bids: None, asks: None })
            .bids = Some(bids);
    }

    fn add_asks(&mut self, key: (Exchange, String), asks: Vec<PriceLevel>) {
        self.partial_books
            .entry(key)
            .or_insert_with(|| PartialOrderBook { bids: None, asks: None })
            .asks = Some(asks);
    }

    fn take_complete(&mut self, key: &(Exchange, String)) -> Option<(Vec<PriceLevel>, Vec<PriceLevel>)> {
        if let Some(partial) = self.partial_books.get(key) {
            if partial.bids.is_some() && partial.asks.is_some() {
                let partial = self.partial_books.remove(key)?;
                return Some((partial.bids?, partial.asks?));
            }
        }
        None
    }
}
