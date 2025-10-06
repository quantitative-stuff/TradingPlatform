mod types;
mod data_ingestion;
mod matching_engine;
mod feature_engineering;
mod oms;

use anyhow::Result;
use data_ingestion::udp_receiver::UdpReceiver;
use matching_engine::MatchingEngine;
use feature_engineering::FeatureEngine;
use oms::OrderManagementSystem;
use tokio::sync::mpsc;
use tracing::{info, error};
use tracing_subscriber;
use types::{Exchange, OrderBookUpdate, Trade, Signal, SignalType, OrderType, Side};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting HFT Pricing Platform");

    // Configuration
    let exchanges = vec![
        (Exchange::Binance, 9001),
        (Exchange::OKX, 9002),
        (Exchange::Bybit, 9003),
    ];

    // Initialize components
    let (mut udp_receiver, mut orderbook_rx, mut trades_rx) =
        UdpReceiver::new(exchanges, 10000);

    let matching_engine = MatchingEngine::new();
    let feature_engine = FeatureEngine::new();
    let oms = OrderManagementSystem::new(
        100000.0,  // max position size
        10000.0,   // max daily loss
        500000.0,  // max total exposure
    );

    // Start UDP receivers
    udp_receiver.start().await?;
    info!("UDP receivers started");

    // Create channels for signals
    let (signal_tx, mut signal_rx) = mpsc::channel::<Signal>(1000);

    // Spawn orderbook processing task
    let matching_engine_clone = matching_engine.clone();
    let feature_engine_clone = feature_engine.clone();
    let signal_tx_clone = signal_tx.clone();

    tokio::spawn(async move {
        while let Some(update) = orderbook_rx.recv().await {
            // Process through matching engine
            match matching_engine_clone.process_l2_update(&update) {
                Ok(events) => {
                    // Update feature engine with new orderbook
                    if let Some(book) = matching_engine_clone.get_order_book(&update.symbol) {
                        feature_engine_clone.update_orderbook(&update.symbol, book.clone());

                        // Calculate features
                        let features = feature_engine_clone.calculate_features(
                            &update.symbol,
                            update.exchange,
                            &book,
                            &[],  // Recent trades would be passed here
                            update.timestamp,
                        );

                        // Generate signal (simplified logic)
                        if let Some(ofi) = features.order_flow_imbalance {
                            if ofi.abs() > 0.3 {  // Threshold for signal generation
                                let signal = Signal {
                                    timestamp: update.timestamp,
                                    symbol: update.symbol.clone(),
                                    exchange: update.exchange,
                                    signal_type: if ofi > 0.0 {
                                        SignalType::Momentum
                                    } else {
                                        SignalType::MeanReversion
                                    },
                                    strength: ofi.abs(),
                                    confidence: 0.7,
                                    features,
                                };

                                if let Err(e) = signal_tx_clone.send(signal).await {
                                    error!("Failed to send signal: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error processing orderbook update: {}", e);
                }
            }
        }
    });

    // Spawn trade processing task
    let feature_engine_clone2 = feature_engine.clone();
    tokio::spawn(async move {
        while let Some(trade) = trades_rx.recv().await {
            feature_engine_clone2.update_trade(&trade.symbol, trade);
        }
    });

    // Spawn signal processing and order generation task
    let oms_clone = oms.clone();
    tokio::spawn(async move {
        while let Some(signal) = signal_rx.recv().await {
            // Simple trading logic based on signals
            if signal.strength > 0.5 && signal.confidence > 0.6 {
                let side = match signal.signal_type {
                    SignalType::Momentum => Side::Buy,
                    SignalType::MeanReversion => Side::Sell,
                    _ => continue,
                };

                // Submit order
                match oms_clone.submit_order(
                    signal.symbol,
                    signal.exchange,
                    side,
                    OrderType::Market,
                    1000.0,  // Fixed quantity for demo
                    None,
                    signal.timestamp,
                ) {
                    Ok(order_id) => {
                        info!("Order submitted: {} for {}", order_id, signal.symbol);
                    }
                    Err(e) => {
                        error!("Failed to submit order: {}", e);
                    }
                }
            }
        }
    });

    // Main loop - monitor performance
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
    loop {
        interval.tick().await;

        let performance = oms.get_performance();
        info!(
            "Performance - Trades: {}, PnL: {:.2}, Win Rate: {:.2}%",
            performance.total_trades,
            performance.total_pnl,
            performance.win_rate * 100.0
        );
    }
}
