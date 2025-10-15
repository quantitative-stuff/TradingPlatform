/// Example of how to use OrderBookBuilder with Binance WebSocket feed
///
/// Run with: cargo run --example binance_orderbook --release

use matching_engine::orderbook_builder::{OrderBookBuilder, OrderBookState};
use matching_engine::monitoring::MonitoringSystem;
use matching_engine::persistence::{PersistenceConfig, PersistenceManager};
use market_types::{OrderBook, OrderBookUpdate, Exchange, PriceLevel};
use tokio::sync::mpsc;
use tracing::{info, error};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Binance OrderBook Builder Example");

    // Create channels for updates and snapshots
    let (update_tx, update_rx) = mpsc::channel::<OrderBookUpdate>(1000);
    let (snapshot_tx, mut snapshot_rx) = mpsc::channel::<OrderBook>(10);

    // Create OrderBook builder
    let symbol = "BTCUSDT".to_string();
    let exchange = Exchange::Binance;

    let builder = OrderBookBuilder::new(
        symbol.clone(),
        exchange,
        update_rx,
        snapshot_tx,
    );

    // Create monitoring system
    let monitor = MonitoringSystem::new(symbol.clone(), format!("{:?}", exchange));

    // Optional: Create persistence manager
    let persistence_config = PersistenceConfig::default();
    let persistence = PersistenceManager::new(
        persistence_config,
        symbol.clone(),
        exchange,
    )?;

    // Start the orderbook builder in background
    let builder_handle = tokio::spawn(async move {
        if let Err(e) = builder.run().await {
            error!("OrderBook builder error: {}", e);
        }
    });

    // Start snapshot receiver in background
    let monitor_clone = monitor.clone();
    tokio::spawn(async move {
        while let Some(snapshot) = snapshot_rx.recv().await {
            info!(
                "Received orderbook snapshot: {} bids, {} asks, seq: {}",
                snapshot.bids.len(),
                snapshot.asks.len(),
                snapshot.last_update_id
            );

            // Record metrics
            monitor_clone.record_snapshot();
        }
    });

    // Simulate receiving updates from Binance WebSocket
    // In production, this would be your actual WebSocket handler
    tokio::spawn(async move {
        // Send initial snapshot (required to transition to Live state)
        let snapshot = OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: "BTCUSDT".to_string(),
            timestamp: chrono::Utc::now().timestamp_micros(),
            bids: vec![
                PriceLevel { price: 50000.0, quantity: 1.5 },
                PriceLevel { price: 49999.0, quantity: 2.0 },
                PriceLevel { price: 49998.0, quantity: 3.0 },
            ],
            asks: vec![
                PriceLevel { price: 50001.0, quantity: 1.5 },
                PriceLevel { price: 50002.0, quantity: 2.0 },
                PriceLevel { price: 50003.0, quantity: 3.0 },
            ],
            is_snapshot: true,
            update_id: 1000,
            first_update_id: 1000,
            prev_update_id: None,
        };

        if let Err(e) = update_tx.send(snapshot).await {
            error!("Failed to send snapshot: {}", e);
            return;
        }

        info!("Sent initial snapshot");

        // Simulate sending incremental updates
        for i in 1..=100 {
            let update = OrderBookUpdate {
                exchange: Exchange::Binance,
                symbol: "BTCUSDT".to_string(),
                timestamp: chrono::Utc::now().timestamp_micros(),
                bids: if i % 3 == 0 {
                    vec![PriceLevel {
                        price: 50000.0 - (i as f64 * 0.1),
                        quantity: 1.0 + (i as f64 * 0.1)
                    }]
                } else {
                    vec![]
                },
                asks: if i % 5 == 0 {
                    vec![PriceLevel {
                        price: 50001.0 + (i as f64 * 0.1),
                        quantity: 1.0 + (i as f64 * 0.05)
                    }]
                } else {
                    vec![]
                },
                is_snapshot: false,
                update_id: 1000 + i,
                first_update_id: 1000 + i,
                prev_update_id: Some(1000 + i - 1),
            };

            if let Err(e) = update_tx.send(update).await {
                error!("Failed to send update: {}", e);
                break;
            }

            // Record metrics
            let start = std::time::Instant::now();
            monitor.record_update(start.elapsed().as_micros() as u64, 256);

            // Small delay to simulate real-time updates
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("Finished sending updates");

        // Simulate a gap to trigger resync
        let gap_update = OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: "BTCUSDT".to_string(),
            timestamp: chrono::Utc::now().timestamp_micros(),
            bids: vec![],
            asks: vec![],
            is_snapshot: false,
            update_id: 1200, // Gap: missing 1101-1199
            first_update_id: 1200,
            prev_update_id: Some(1199),
        };

        if let Err(e) = update_tx.send(gap_update).await {
            error!("Failed to send gap update: {}", e);
        }

        info!("Sent update with gap to trigger resync");

        // Wait a bit then send recovery snapshot
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let recovery = OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: "BTCUSDT".to_string(),
            timestamp: chrono::Utc::now().timestamp_micros(),
            bids: vec![
                PriceLevel { price: 51000.0, quantity: 2.0 },
                PriceLevel { price: 50999.0, quantity: 3.0 },
            ],
            asks: vec![
                PriceLevel { price: 51001.0, quantity: 2.0 },
                PriceLevel { price: 51002.0, quantity: 3.0 },
            ],
            is_snapshot: true,
            update_id: 1200,
            first_update_id: 1200,
            prev_update_id: None,
        };

        if let Err(e) = update_tx.send(recovery).await {
            error!("Failed to send recovery snapshot: {}", e);
        }

        info!("Sent recovery snapshot");
    });

    // Monitor health periodically
    let monitor_clone = monitor.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            let health = monitor_clone.health_check(10000);
            info!("Health check: {:?}", health.status);

            let metrics = monitor_clone.get_metrics();
            info!(
                "Metrics: {} updates, {} gaps, {:.2} updates/sec",
                metrics.total_updates_processed,
                metrics.gaps_detected,
                metrics.updates_per_second
            );
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}