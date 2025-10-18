/// HFT OrderBook Consumer Binary
///
/// Connects UDP Receiver → HFT OrderBook Processor
///
/// This binary:
/// 1. Receives UDP packets from the feeder (binary protocol)
/// 2. Pushes orderbook updates to lock-free ring buffers
/// 3. Processing thread applies updates to fast orderbooks
/// 4. Exposes orderbooks for feature engineering and signal generation

use anyhow::{Result, Context};
use matching_engine::hft_orderbook::HFTOrderBookProcessor;
use market_types::Exchange;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::fs;
use std::str::FromStr;
use std::collections::HashSet;
use tracing::{info, warn, debug};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Import from udp-protocol crate
use udp_protocol::UdpReceiver;

// Configuration structures
#[derive(Debug, Deserialize, Serialize)]
struct ConsumerConfig {
    udp_config: UdpConfig,
    symbols: Vec<SymbolConfig>,
    processing_config: ProcessingConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct UdpConfig {
    port: u16,
    buffer_size: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct SymbolConfig {
    exchange: String,
    symbol: String,
    tick_size: f64,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ProcessingConfig {
    cpu_core: usize,
    ring_buffer_size: usize,
    batch_size: usize,
    max_symbols: usize,
}

fn load_config(path: &str) -> Result<ConsumerConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path))?;

    let config: ConsumerConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path))?;

    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "consumer=info,matching_engine=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("🚀 Starting HFT OrderBook Consumer");

    // ========================================
    // Step 1: Load configuration
    // ========================================
    let config_path = std::env::var("CONSUMER_CONFIG")
        .unwrap_or_else(|_| "config/consumer/symbols.json".to_string());

    info!("📄 Loading config from: {}", config_path);
    let config = load_config(&config_path)?;

    info!("✓ Loaded {} symbols from config", config.symbols.len());
    info!("✓ UDP port: {}, buffer size: {}", config.udp_config.port, config.udp_config.buffer_size);

    // ========================================
    // Step 2: Create HFT OrderBook Processor
    // ========================================
    let mut processor = HFTOrderBookProcessor::new();

    // ========================================
    // Step 3: Register symbols from config
    // ========================================
    let mut enabled_symbols = Vec::new();
    for symbol_cfg in &config.symbols {
        if !symbol_cfg.enabled {
            debug!("Skipping disabled symbol: {} on {}", symbol_cfg.symbol, symbol_cfg.exchange);
            continue;
        }

        // Parse exchange
        let exchange = Exchange::from_str(&symbol_cfg.exchange)
            .with_context(|| format!("Unknown exchange: {}", symbol_cfg.exchange))?;

        // Create unique symbol identifier with exchange
        let unique_symbol = format!("{}-{}", symbol_cfg.symbol, exchange);

        processor.register_symbol(&unique_symbol, symbol_cfg.tick_size);
        enabled_symbols.push((symbol_cfg.symbol.clone(), exchange));
    }

    info!("📝 Registered {} enabled symbols", enabled_symbols.len());

    // ========================================
    // Step 3: Start processing thread
    // ========================================
    info!("🔧 Starting HFT processing thread");
    processor.start_processing();

    // Keep processor alive for the main loop
    let processor = Arc::new(processor);

    // ========================================
    // Step 4: Create UDP Receiver
    // ========================================
    info!("📡 Creating UDP receiver");

    // Get unique exchanges from enabled symbols
    let unique_exchanges: HashSet<Exchange> = enabled_symbols
        .iter()
        .map(|(_, exchange)| *exchange)
        .collect();

    // Create exchanges list with configured port
    let exchanges: Vec<(Exchange, u16)> = unique_exchanges
        .iter()
        .map(|ex| (*ex, config.udp_config.port))
        .collect();

    info!("📡 Listening for {} exchanges on port {}", exchanges.len(), config.udp_config.port);

    let (mut receiver, mut orderbook_rx, mut trade_rx) =
        UdpReceiver::new(exchanges, config.udp_config.buffer_size);

    // ========================================
    // Step 5: Start UDP receiver
    // ========================================
    info!("🎧 Starting UDP receiver");
    receiver.start().await?;

    info!("✅ Consumer ready - waiting for UDP packets");

    // ========================================
    // Step 6: Main event loop
    // ========================================
    let mut orderbook_count = 0u64;
    let mut trade_count = 0u64;
    let mut last_stats = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(update) = orderbook_rx.recv() => {
                orderbook_count += 1;

                // Create unique symbol identifier with exchange
                let unique_symbol = format!("{}-{}", update.symbol, update.exchange);

                // Convert to FastUpdate
                match processor.convert_update(&update) {
                    Some(fast_update) => {
                        // Get ring buffer for this exchange
                        let ring = processor.get_ring_buffer(update.exchange);

                        // Push to ring buffer
                        if !ring.push(fast_update) {
                            warn!("⚠️  Ring buffer full for {} ({})", update.exchange, unique_symbol);
                        } else {
                            debug!("✓ Pushed update for {} to ring buffer", unique_symbol);
                        }
                    }
                    None => {
                        warn!("⚠️  Failed to convert update for {}", unique_symbol);
                    }
                }
            }
            Some(trade) = trade_rx.recv() => {
                trade_count += 1;

                // For now, just count trades
                // TODO: Send to feature engineering module
                debug!("📊 Trade: {} {} @ {} (qty: {})",
                    trade.exchange, trade.symbol, trade.price, trade.quantity);
            }
        }

        // Log stats every 5 seconds
        if last_stats.elapsed().as_secs() >= 5 {
            let elapsed = last_stats.elapsed().as_secs_f64();
            let orderbook_rate = orderbook_count as f64 / elapsed;
            let trade_rate = trade_count as f64 / elapsed;

            info!("📈 Stats: {:.0} orderbooks/sec, {:.0} trades/sec",
                orderbook_rate, trade_rate);

            orderbook_count = 0;
            trade_count = 0;
            last_stats = std::time::Instant::now();
        }
    }
}
