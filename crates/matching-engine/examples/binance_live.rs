/// Live Binance WebSocket integration with OrderBookBuilder
///
/// This connects to real Binance WebSocket endpoints and processes live market data
/// Run with: cargo run --example binance_live --release

use matching_engine::orderbook_builder::OrderBookBuilder;
use matching_engine::monitoring::MonitoringSystem;
use matching_engine::persistence::{PersistenceConfig, PersistenceManager};
use market_types::{OrderBook, OrderBookUpdate, Exchange, PriceLevel};
use tokio::sync::mpsc;
use tracing::{info, error, warn};
use anyhow::{Result, Context};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::Value;
use std::time::{Duration, Instant};

const BINANCE_WS_URL: &str = "wss://stream.binance.com:9443/ws";

/// Parse Binance depth update message into OrderBookUpdate
fn parse_depth_update(msg: &Value, symbol: &str) -> Result<OrderBookUpdate> {
    let event_type = msg["e"].as_str().unwrap_or("");

    let update = if event_type == "depthUpdate" {
        // Incremental update
        let bids = msg["b"].as_array()
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

        let asks = msg["a"].as_array()
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

        OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: symbol.to_string(),
            timestamp: msg["E"].as_i64().unwrap_or_else(|| {
                chrono::Utc::now().timestamp_micros()
            }),
            bids,
            asks,
            is_snapshot: false,
            update_id: msg["u"].as_u64().unwrap_or(0),
            first_update_id: msg["U"].as_u64().unwrap_or(0),
            prev_update_id: Some(msg["pu"].as_u64().unwrap_or(0)),
        }
    } else {
        // Unknown message type
        return Err(anyhow::anyhow!("Unknown message type: {}", event_type));
    };

    Ok(update)
}

/// Fetch REST snapshot from Binance
async fn fetch_snapshot(symbol: &str, limit: u32) -> Result<OrderBookUpdate> {
    let url = format!(
        "https://api.binance.com/api/v3/depth?symbol={}&limit={}",
        symbol.to_uppercase(),
        limit
    );

    let response = reqwest::get(&url).await?
        .json::<Value>().await?;

    let bids = response["bids"].as_array()
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

    let asks = response["asks"].as_array()
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

    Ok(OrderBookUpdate {
        exchange: Exchange::Binance,
        symbol: symbol.to_uppercase(),
        timestamp: chrono::Utc::now().timestamp_micros(),
        bids,
        asks,
        is_snapshot: true,
        update_id: response["lastUpdateId"].as_u64().unwrap_or(0),
        first_update_id: response["lastUpdateId"].as_u64().unwrap_or(0),
        prev_update_id: None,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Live Binance OrderBook Builder");

    // Configuration
    let symbol = "BTCUSDT";
    let exchange = Exchange::Binance;

    // Create channels
    let (update_tx, update_rx) = mpsc::channel::<OrderBookUpdate>(1000);
    let (snapshot_tx, mut snapshot_rx) = mpsc::channel::<OrderBook>(10);

    // Create OrderBook builder
    let builder = OrderBookBuilder::new(
        symbol.to_string(),
        exchange,
        update_rx,
        snapshot_tx,
    );

    // Create monitoring system
    let monitor = MonitoringSystem::new(symbol.to_string(), format!("{:?}", exchange));

    // Optional: Create persistence manager
    let persistence_config = PersistenceConfig::default();
    let persistence = PersistenceManager::new(
        persistence_config,
        symbol.to_string(),
        exchange,
    )?;

    // Start the orderbook builder
    let builder_handle = tokio::spawn(async move {
        if let Err(e) = builder.run().await {
            error!("OrderBook builder error: {}", e);
        }
    });

    // Start snapshot consumer
    let monitor_clone = monitor.clone();
    let snapshot_consumer = tokio::spawn(async move {
        while let Some(snapshot) = snapshot_rx.recv().await {
            info!(
                "📚 OrderBook Snapshot: {} bids, {} asks, seq: {}, spread: {:.2}",
                snapshot.bids.len(),
                snapshot.asks.len(),
                snapshot.last_update_id,
                snapshot.asks.first().map(|a| a.price).unwrap_or(0.0) -
                    snapshot.bids.first().map(|b| b.price).unwrap_or(0.0)
            );

            // Record metrics
            monitor_clone.record_snapshot();

            // Log top of book
            if let (Some(best_bid), Some(best_ask)) =
                (snapshot.bids.first(), snapshot.asks.first()) {
                info!(
                    "  Best Bid: {:.2} @ {:.4}, Best Ask: {:.2} @ {:.4}",
                    best_bid.price, best_bid.quantity,
                    best_ask.price, best_ask.quantity
                );
            }
        }
    });

    // Connect to Binance WebSocket
    let ws_url = format!("{}/{}@depth@100ms", BINANCE_WS_URL, symbol.to_lowercase());
    info!("Connecting to Binance WebSocket: {}", ws_url);

    let (ws_stream, _) = connect_async(&ws_url).await
        .context("Failed to connect to Binance WebSocket")?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Send initial REST snapshot to bootstrap the orderbook
    info!("Fetching initial REST snapshot...");
    match fetch_snapshot(symbol, 1000).await {
        Ok(snapshot) => {
            info!("Got REST snapshot with {} bids, {} asks, lastUpdateId: {}",
                snapshot.bids.len(), snapshot.asks.len(), snapshot.update_id);

            if let Err(e) = update_tx.send(snapshot).await {
                error!("Failed to send initial snapshot: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to fetch initial snapshot: {}", e);
            // Continue anyway - builder will request snapshot when needed
        }
    }

    // Process WebSocket messages
    let monitor_clone2 = monitor.clone();
    let ws_processor = tokio::spawn(async move {
        let mut message_count = 0u64;
        let mut last_log = Instant::now();

        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    message_count += 1;

                    // Parse JSON
                    match serde_json::from_str::<Value>(&text) {
                        Ok(json) => {
                            // Parse depth update
                            match parse_depth_update(&json, symbol) {
                                Ok(update) => {
                                    let start = Instant::now();

                                    // Send to orderbook builder
                                    if let Err(e) = update_tx.send(update).await {
                                        error!("Failed to send update: {}", e);
                                        break;
                                    }

                                    // Record metrics
                                    let latency = start.elapsed().as_micros() as u64;
                                    monitor_clone2.record_update(latency, text.len() as u64);
                                }
                                Err(e) => {
                                    warn!("Failed to parse depth update: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse WebSocket message: {}", e);
                        }
                    }

                    // Log stats periodically
                    if last_log.elapsed() > Duration::from_secs(10) {
                        info!("📊 Processed {} WebSocket messages", message_count);

                        let metrics = monitor_clone2.get_metrics();
                        info!(
                            "  Updates: {}, Snapshots: {}, Rate: {:.1}/s, Avg Latency: {:.0}μs",
                            metrics.total_updates_processed,
                            metrics.snapshots_received,
                            metrics.updates_per_second,
                            metrics.avg_update_latency
                        );

                        last_log = Instant::now();
                    }
                }
                Ok(Message::Ping(data)) => {
                    // Respond to ping
                    if let Err(e) = ws_sender.send(Message::Pong(data)).await {
                        error!("Failed to send pong: {}", e);
                        break;
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        info!("WebSocket processor ended");
    });

    // Health monitoring
    let monitor_clone3 = monitor.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;

            let health = monitor_clone3.health_check(60000);
            info!("🏥 Health Status: {:?}", health.status);

            if !health.checks.iter().all(|c| c.passed) {
                let failed_checks: Vec<_> = health.checks.iter()
                    .filter(|c| !c.passed)
                    .map(|c| format!("{}: {}", c.name, c.message))
                    .collect();
                warn!("Health Issues: {:?}", failed_checks);
            }
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Graceful shutdown
    ws_processor.abort();
    snapshot_consumer.abort();
    builder_handle.abort();

    Ok(())
}