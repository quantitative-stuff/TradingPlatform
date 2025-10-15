/// Direct HFT Receiver with Feature Pipeline
///
/// Directly accesses the HFT orderbook processor on the SAME MACHINE as the feeder
/// Bypasses network/UDP completely for ultra-low latency
///
/// This is useful when you want to:
/// - Run trading strategies on the feeder machine itself
/// - Test strategies without network overhead
/// - Get the absolute lowest latency possible (microseconds)
///
/// Usage:
///   cargo run --bin direct_hft_receiver --release

use anyhow::Result;
use tokio::time::{interval, Duration};
use tracing::{info, debug, warn, error};
use feeder::core::{get_orderbook_snapshot, get_hft_processor};
use feature_engineering::pipeline::FeaturePipeline;
use market_types::{OrderBookUpdate, Exchange, PriceLevel};
use std::collections::HashMap;
use std::sync::Arc;

const LOOKBACK_MS: i64 = 1000; // 1 second lookback for features

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!("╔════════════════════════════════════════════════════════╗");
    info!("║     Direct HFT Receiver with Feature Pipeline         ║");
    info!("╠════════════════════════════════════════════════════════╣");
    info!("║  Direct Memory Access → Features → Trading            ║");
    info!("║                                                        ║");
    info!("║  Advantages:                                           ║");
    info!("║    • No network overhead                              ║");
    info!("║    • Microsecond latency                              ║");
    info!("║    • Zero serialization cost                          ║");
    info!("║    • Direct access to HFT orderbooks                  ║");
    info!("║                                                        ║");
    info!("║  NOTE: Must run on same machine as feeder_direct      ║");
    info!("╚════════════════════════════════════════════════════════╝");

    // Check if HFT processor is available
    if get_hft_processor().is_none() {
        error!("HFT processor not initialized!");
        error!("Please ensure feeder_direct is running on this machine first.");
        return Err(anyhow::anyhow!("HFT processor not available"));
    }
    info!("✅ Connected to HFT processor");

    // Create feature pipeline
    let pipeline = Arc::new(FeaturePipeline::new(LOOKBACK_MS));
    info!("✅ Feature pipeline initialized (lookback: {}ms)", LOOKBACK_MS);

    // Symbols to monitor (you can load from config)
    let symbols = vec![
        ("BTCUSDT", Exchange::Binance),
        ("ETHUSDT", Exchange::Binance),
        ("BNBUSDT", Exchange::Binance),
        ("SOLUSDT", Exchange::Binance),
        ("XRPUSDT", Exchange::Binance),
        ("BTCUSDT", Exchange::Bybit),
        ("ETHUSDT", Exchange::Bybit),
        ("BTCUSDT", Exchange::OKX),
        ("ETHUSDT", Exchange::OKX),
    ];

    // Track last update IDs to detect changes
    let mut last_update_ids: HashMap<String, u64> = HashMap::new();
    let mut feature_count = 0u64;
    let mut last_stats_time = std::time::Instant::now();

    // Main processing loop - poll at high frequency
    let mut tick_interval = interval(Duration::from_micros(100)); // 10kHz polling

    info!("Starting direct HFT processing loop...");
    info!("Polling {} symbols at 10kHz", symbols.len());

    loop {
        tick_interval.tick().await;

        for (symbol, exchange) in &symbols {
            // Direct memory access to HFT orderbook
            if let Some(snapshot) = get_orderbook_snapshot(symbol) {
                // Check if orderbook updated
                let last_id = last_update_ids.get(*symbol).copied().unwrap_or(0);
                if snapshot.last_update_id > last_id {
                    last_update_ids.insert(symbol.to_string(), snapshot.last_update_id);

                    // Convert HFT snapshot to OrderBookUpdate for feature pipeline
                    let update = convert_to_update(&snapshot, *exchange, symbol.to_string());

                    // Process through feature pipeline
                    match pipeline.process_order_book_update(update) {
                        Ok(Some(features)) => {
                            feature_count += 1;

                            // Process trading signals
                            if let (Some(ofi), Some(spread)) =
                                (features.order_flow_imbalance, features.spread) {

                                // Example trading logic
                                if ofi.abs() > 0.7 && spread < 0.0005 {
                                    info!(
                                        "🎯 SIGNAL [{}:{}] OFI={:.3} Spread={:.5} WAP={:.2}",
                                        exchange, symbol, ofi, spread,
                                        features.wap_0ms.unwrap_or(0.0)
                                    );

                                    // TODO: Execute trade here
                                    // e.g., send_order(symbol, side, quantity, price)
                                }
                            }

                            // Log stats every 10 seconds
                            if last_stats_time.elapsed().as_secs() >= 10 {
                                info!(
                                    "Stats: {} features/sec, Last [{}:{}] OFI={:.4} Spread={:.4}",
                                    feature_count / 10,
                                    features.exchange,
                                    features.symbol,
                                    features.order_flow_imbalance.unwrap_or(0.0),
                                    features.spread.unwrap_or(0.0),
                                );

                                // Show HFT orderbook depth
                                info!(
                                    "  HFT OrderBook {}: {} bids x {} asks (seq {})",
                                    symbol,
                                    snapshot.bids.len(),
                                    snapshot.asks.len(),
                                    snapshot.last_update_id
                                );

                                feature_count = 0;
                                last_stats_time = std::time::Instant::now();
                            }
                        }
                        Ok(None) => {
                            // Not enough data for features yet
                        }
                        Err(e) => {
                            error!("Error processing features: {}", e);
                        }
                    }
                }
            }
        }

        // Check for shutdown
        if tokio::signal::ctrl_c().is_pending() {
            info!("Received shutdown signal");
            break;
        }
    }

    info!("Direct HFT receiver stopped");
    Ok(())
}

/// Convert HFT snapshot to OrderBookUpdate for feature pipeline
fn convert_to_update(
    snapshot: &feeder::core::FastOrderBookSnapshot,
    exchange: Exchange,
    symbol: String,
) -> OrderBookUpdate {
    // Convert integer ticks back to floating point prices
    let bids: Vec<PriceLevel> = snapshot.bids.iter()
        .map(|level| PriceLevel {
            price: level.price_ticks as f64 * snapshot.tick_size,
            quantity: level.quantity as f64 / 1e8,
        })
        .collect();

    let asks: Vec<PriceLevel> = snapshot.asks.iter()
        .map(|level| PriceLevel {
            price: level.price_ticks as f64 * snapshot.tick_size,
            quantity: level.quantity as f64 / 1e8,
        })
        .collect();

    OrderBookUpdate {
        exchange,
        symbol,
        timestamp: chrono::Utc::now().timestamp_micros(),
        bids,
        asks,
        is_snapshot: true,
        update_id: snapshot.last_update_id,
        first_update_id: snapshot.last_update_id,
        prev_update_id: None,
    }
}

/// Example function to check for Ctrl+C without blocking
trait SignalCheck {
    fn is_pending(&self) -> bool;
}

impl SignalCheck for tokio::signal::unix::Signal {
    fn is_pending(&self) -> bool {
        false // Simplified - in production use proper signal handling
    }
}

// For Windows compatibility
#[cfg(windows)]
impl SignalCheck for () {
    fn is_pending(&self) -> bool {
        false
    }
}