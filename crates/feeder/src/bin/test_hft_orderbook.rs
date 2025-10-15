/// Simple test program to verify HFT orderbooks are working
///
/// Usage: cargo run --bin test_hft_orderbook --release

use anyhow::Result;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};
use feeder::core::hft_integration::{get_orderbook_snapshot, get_hft_processor, init_hft_processor, start_hft_processor};
use std::process;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!("HFT OrderBook Test Program");
    info!("==========================");

    // Check if HFT processor is available (from running feeder_direct)
    if get_hft_processor().is_none() {
        warn!("HFT processor not found. Starting feeder_direct first...");

        // Start feeder_direct as child process
        let mut child = process::Command::new("./target/release/feeder_direct.exe")
            .spawn()
            .expect("Failed to start feeder_direct");

        info!("Started feeder_direct (PID: {})", child.id());
        info!("Waiting for initialization...");
        sleep(Duration::from_secs(10)).await;

        // Check again
        if get_hft_processor().is_none() {
            error!("Still no HFT processor after starting feeder_direct");
            child.kill().ok();
            return Err(anyhow::anyhow!("Could not connect to HFT processor"));
        }
    }

    info!("Connected to HFT processor!");
    info!("Monitoring orderbooks...\n");

    // List of key symbols to monitor
    let symbols = vec![
        "BTCUSDT",
        "ETHUSDT",
        "BNBUSDT",
        "SOLUSDT",
        "XRPUSDT",
        "DOGEUSDT",
        "ADAUSDT",
        "AVAXUSDT",
        "SHIBUSDT",
        "DOTUSDT",
    ];

    let mut iteration = 0;

    loop {
        iteration += 1;
        info!("═══════════════════════════════════════════════════");
        info!("Iteration {} - Checking orderbooks", iteration);
        info!("═══════════════════════════════════════════════════");

        let mut found_any = false;

        for symbol in &symbols {
            if let Some(snapshot) = get_orderbook_snapshot(symbol) {
                if !snapshot.bids.is_empty() || !snapshot.asks.is_empty() {
                    found_any = true;

                    // Get best bid and ask
                    let best_bid = if !snapshot.bids.is_empty() {
                        let price = snapshot.bids[0].price_ticks as f64 * snapshot.tick_size;
                        let qty = snapshot.bids[0].quantity as f64 / 1e8;
                        format!("{:.2} ({:.4})", price, qty)
                    } else {
                        "---".to_string()
                    };

                    let best_ask = if !snapshot.asks.is_empty() {
                        let price = snapshot.asks[0].price_ticks as f64 * snapshot.tick_size;
                        let qty = snapshot.asks[0].quantity as f64 / 1e8;
                        format!("{:.2} ({:.4})", price, qty)
                    } else {
                        "---".to_string()
                    };

                    let spread = if !snapshot.bids.is_empty() && !snapshot.asks.is_empty() {
                        let bid_price = snapshot.bids[0].price_ticks as f64 * snapshot.tick_size;
                        let ask_price = snapshot.asks[0].price_ticks as f64 * snapshot.tick_size;
                        format!("{:.4}", ask_price - bid_price)
                    } else {
                        "---".to_string()
                    };

                    info!(
                        "{:<10} Bid: {:<20} Ask: {:<20} Spread: {:<10} Depth: {}x{} Seq: {}",
                        symbol,
                        best_bid,
                        best_ask,
                        spread,
                        snapshot.bids.len(),
                        snapshot.asks.len(),
                        snapshot.last_update_id
                    );
                }
            }
        }

        if !found_any {
            warn!("No orderbooks with data found yet. Feeder may still be connecting...");

            // Check if processor exists
            if get_hft_processor().is_some() {
                info!("HFT processor is running, waiting for data...");
            } else {
                error!("HFT processor disappeared!");
                break;
            }
        } else {
            info!("\nFound {} symbols with orderbook data",
                  symbols.iter().filter(|s| {
                      get_orderbook_snapshot(s)
                          .map(|snap| !snap.bids.is_empty() || !snap.asks.is_empty())
                          .unwrap_or(false)
                  }).count());
        }

        // Wait before next iteration
        sleep(Duration::from_secs(5)).await;
    }

    Ok(())
}