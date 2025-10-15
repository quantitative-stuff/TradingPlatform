/// HFT Direct OrderBook Receiver
///
/// Directly accesses the in-memory HFT orderbook processor
/// for ultra-low latency trading strategies
///
/// This receiver runs on the SAME machine as the feeder

use anyhow::Result;
use tokio::time::{interval, Duration};
use tracing::{info, debug, warn};
use feeder::core::get_orderbook_snapshot;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting HFT Direct OrderBook Receiver");

    // Symbols to monitor (you can load from config)
    let symbols = vec![
        "BTCUSDT".to_string(),
        "ETHUSDT".to_string(),
        "BNBUSDT".to_string(),
        "SOLUSDT".to_string(),
        "XRPUSDT".to_string(),
    ];

    // Main processing loop
    let mut tick_interval = interval(Duration::from_millis(1)); // 1ms = 1000 Hz
    let mut last_update_ids: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    loop {
        tick_interval.tick().await;

        for symbol in &symbols {
            // Direct memory access to HFT orderbook
            if let Some(snapshot) = get_orderbook_snapshot(symbol) {
                // Check if orderbook updated
                let last_id = last_update_ids.get(symbol).copied().unwrap_or(0);
                if snapshot.last_update_id > last_id {
                    last_update_ids.insert(symbol.clone(), snapshot.last_update_id);

                    // Process the orderbook (example: calculate spread)
                    if !snapshot.bids.is_empty() && !snapshot.asks.is_empty() {
                        let best_bid = snapshot.bids[0].price_ticks as f64 * snapshot.tick_size;
                        let best_ask = snapshot.asks[0].price_ticks as f64 * snapshot.tick_size;
                        let spread = best_ask - best_bid;
                        let spread_bps = (spread / best_bid) * 10000.0;

                        debug!(
                            "{}: Bid {:.2} Ask {:.2} Spread {:.2} ({:.1} bps) Seq {}",
                            symbol, best_bid, best_ask, spread, spread_bps, snapshot.last_update_id
                        );

                        // ===== ADD YOUR TRADING LOGIC HERE =====
                        // Example: Check for arbitrage opportunities
                        if spread_bps < 1.0 {  // Tight spread
                            // Potential opportunity
                            process_trading_signal(symbol, &snapshot);
                        }
                    }
                }
            } else {
                // OrderBook not yet initialized for this symbol
                debug!("No orderbook for {} yet", symbol);
            }
        }
    }
}

fn process_trading_signal(symbol: &str, snapshot: &feeder::core::FastOrderBookSnapshot) {
    // Your HFT strategy logic here
    // Examples:
    // - Market making: Place orders around the spread
    // - Arbitrage: Compare with other exchanges
    // - Momentum: Detect rapid price moves
    // - Mean reversion: Identify oversold/overbought

    info!("Trading signal for {}: {} bids, {} asks",
          symbol, snapshot.bids.len(), snapshot.asks.len());
}