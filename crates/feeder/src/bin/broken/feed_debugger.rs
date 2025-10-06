use feeder::core::{SymbolMapper, TRADES, ORDERBOOKS, CONNECTION_STATS};
use feeder::core::Feeder;
use feeder::crypto::feed::{
    binance::BinanceExchange,
    bybit::BybitExchange,
    okx::OkxExchange,
    coinbase::CoinbaseExchange,
    upbit::UpbitExchange,
    deribit::DeribitExchange,
    bithumb::BithumbExchange,
};
use feeder::load_config::ExchangeConfig;
use feeder::error::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, interval};
use tracing::{info, warn, error, debug, Level};
use std::collections::HashMap;
use feeder::core::Feeder;
use feeder::core::Feeder;
use clap::{Arg, Command};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let matches = Command::new("Feed Debugger")
        .version("1.0")
        .about("Debug WebSocket feeds from crypto exchanges")
        .arg(
            Arg::new("exchange")
                .short('e')
                .long("exchange")
                .value_name("EXCHANGE")
                .help("Exchange to debug (bybit, binance, okx, coinbase, upbit, deribit, bithumb, all)")
                .required(true)
        )
        .arg(
            Arg::new("symbols")
                .short('s')
                .long("symbols")
                .value_name("COUNT")
                .help("Number of symbols to test (default: 3)")
                .default_value("3")
        )
        .arg(
            Arg::new("duration")
                .short('d')
                .long("duration")
                .value_name("SECONDS")
                .help("Duration to run debug session (default: 60)")
                .default_value("60")
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Enable verbose logging")
                .action(clap::ArgAction::SetTrue)
        )
        .get_matches();

    // Initialize logging
    let log_level = if matches.get_flag("verbose") {
        Level::DEBUG
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    let exchange = matches.get_one::<String>("exchange").unwrap();
    let symbol_count: usize = matches.get_one::<String>("symbols").unwrap().parse().unwrap_or(3);
    let duration: u64 = matches.get_one::<String>("duration").unwrap().parse().unwrap_or(60);

    info!("🔍 Feed Debugger Starting");
    info!("   Exchange: {}", exchange);
    info!("   Symbol count: {}", symbol_count);
    info!("   Duration: {} seconds", duration);
    info!("   Log level: {:?}", log_level);

    // Initialize symbol mapper
    let symbol_mapper = Arc::new(SymbolMapper::new());

    // Start monitoring in background
    let monitor_handle = tokio::spawn(monitor_data_flow(duration));

    // Start exchange debugging
    match exchange.to_lowercase().as_str() {
        "bybit" => debug_bybit(symbol_mapper, symbol_count).await?,
        "binance" => debug_binance(symbol_mapper, symbol_count).await?,
        "okx" => debug_okx(symbol_mapper, symbol_count).await?,
        "coinbase" => debug_coinbase(symbol_mapper, symbol_count).await?,
        "upbit" => debug_upbit(symbol_mapper, symbol_count).await?,
        "deribit" => debug_deribit(symbol_mapper, symbol_count).await?,
        "bithumb" => debug_bithumb(symbol_mapper, symbol_count).await?,
        "all" => {
            error!("❌ 'all' mode not implemented in this version. Please run individual exchanges.");
            return Err(Error::Config("Not implemented".to_string()));
        },
        _ => {
            error!("❌ Unknown exchange: {}", exchange);
            return Err(Error::Config("Invalid exchange".to_string()));
        }
    }

    // Wait for monitoring to complete
    let _ = monitor_handle.await;

    info!("🔍 Feed Debugger Completed");
    Ok(())
}

async fn debug_bybit(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging Bybit exchange...");

    let config = load_exchange_config("config/crypto/bybit_config.json", symbol_count)?;
    log_config_details(&config, "Bybit");

    let mut exchange = BybitExchange::new(config, symbol_mapper);

    info!("📡 Connecting to Bybit...");
    exchange.connect().await?;

    info!("📝 Subscribing to Bybit streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting Bybit data stream...");
    exchange.start().await?;

    Ok(())
}

async fn debug_binance(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging Binance exchange...");

    let config = load_exchange_config("config/crypto/binance_config.json", symbol_count)?;
    log_config_details(&config, "Binance");

    let mut exchange = BinanceExchange::new(config, symbol_mapper);

    info!("📡 Connecting to Binance...");
    exchange.connect().await?;

    info!("📝 Subscribing to Binance streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting Binance data stream...");
    exchange.start().await?;

    Ok(())
}

async fn debug_okx(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging OKX exchange...");

    let config = load_exchange_config("config/crypto/okx_config.json", symbol_count)?;
    log_config_details(&config, "OKX");

    let mut exchange = OkxExchange::new(config, symbol_mapper);

    info!("📡 Connecting to OKX...");
    exchange.connect().await?;

    info!("📝 Subscribing to OKX streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting OKX data stream...");
    exchange.start().await?;

    Ok(())
}

async fn debug_coinbase(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging Coinbase exchange...");

    let config = load_exchange_config("config/crypto/coinbase_config.json", symbol_count)?;
    log_config_details(&config, "Coinbase");

    let mut exchange = CoinbaseExchange::new(config, symbol_mapper);

    info!("📡 Connecting to Coinbase...");
    exchange.connect().await?;

    info!("📝 Subscribing to Coinbase streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting Coinbase data stream...");
    exchange.start().await?;

    Ok(())
}

async fn debug_upbit(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging Upbit exchange...");

    let config = load_exchange_config("config/crypto/upbit_config.json", symbol_count)?;
    log_config_details(&config, "Upbit");

    let mut exchange = UpbitExchange::new(config, symbol_mapper);

    info!("📡 Connecting to Upbit...");
    exchange.connect().await?;

    info!("📝 Subscribing to Upbit streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting Upbit data stream...");
    exchange.start().await?;

    Ok(())
}

async fn debug_deribit(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging Deribit exchange...");

    let config = load_exchange_config("config/crypto/deribit_config.json", symbol_count)?;
    log_config_details(&config, "Deribit");

    let mut exchange = DeribitExchange::new(config, symbol_mapper);

    info!("📡 Connecting to Deribit...");
    exchange.connect().await?;

    info!("📝 Subscribing to Deribit streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting Deribit data stream...");
    exchange.start().await?;

    Ok(())
}

async fn debug_bithumb(symbol_mapper: Arc<SymbolMapper>, symbol_count: usize) -> Result<()> {
    info!("🔍 Debugging Bithumb exchange...");

    let config = load_exchange_config("config/crypto/bithumb_config.json", symbol_count)?;
    log_config_details(&config, "Bithumb");

    let mut exchange = BithumbExchange::new(config, symbol_mapper);

    info!("📡 Connecting to Bithumb...");
    exchange.connect().await?;

    info!("📝 Subscribing to Bithumb streams...");
    exchange.subscribe().await?;

    info!("🚀 Starting Bithumb data stream...");
    exchange.start().await?;

    Ok(())
}

fn load_exchange_config(path: &str, symbol_count: usize) -> Result<ExchangeConfig> {
    info!("📁 Loading config from: {}", path);

    let config_str = std::fs::read_to_string(path)?;
    let mut config: ExchangeConfig = serde_json::from_str(&config_str)?;

    // Limit symbols for debugging
    if config.subscribe_data.codes.len() > symbol_count {
        info!("✂️  Limiting symbols from {} to {} for debugging",
              config.subscribe_data.codes.len(), symbol_count);
        config.subscribe_data.codes.truncate(symbol_count);
    }

    Ok(config)
}

fn log_config_details(config: &ExchangeConfig, exchange_name: &str) {
    info!("📋 {} Configuration:", exchange_name);
    info!("   Exchange: {}", config.feed_config.exchange);
    info!("   Asset types: {:?}", config.feed_config.asset_type);
    info!("   Symbols: {:?}", config.subscribe_data.codes);
    info!("   Stream types: {:?}", config.subscribe_data.stream_type);
    info!("   Order depth: {}", config.subscribe_data.order_depth);
    info!("   WebSocket URL: {}", config.connect_config.ws_url);
    info!("   Max connections: {}", config.connect_config.max_connections);
    info!("   Connection delay: {}ms", config.connect_config.connection_delay_ms);
    info!("   Retry delays: {}s - {}s",
          config.connect_config.initial_retry_delay_secs,
          config.connect_config.max_retry_delay_secs);
}

async fn monitor_data_flow(duration_secs: u64) {
    info!("📊 Starting data flow monitoring for {} seconds...", duration_secs);

    let mut interval = interval(Duration::from_secs(5));
    let mut elapsed = 0u64;

    let mut last_trade_count = 0usize;
    let mut last_orderbook_count = 0usize;

    loop {
        interval.tick().await;
        elapsed += 5;

        // Get current counts
        let trade_count = TRADES.read().len();
        let orderbook_count = ORDERBOOKS.read().len();

        // Calculate rates
        let trade_rate = (trade_count - last_trade_count) as f64 / 5.0;
        let orderbook_rate = (orderbook_count - last_orderbook_count) as f64 / 5.0;

        info!("📊 Data Flow Report ({}/{}s):", elapsed, duration_secs);
        info!("   📈 Trades: {} total (+{} in 5s, {:.1}/s)",
              trade_count, trade_count - last_trade_count, trade_rate);
        info!("   📖 Orderbooks: {} total (+{} in 5s, {:.1}/s)",
              orderbook_count, orderbook_count - last_orderbook_count, orderbook_rate);

        // Log connection statistics
        {
            let stats = CONNECTION_STATS.read();
            if !stats.is_empty() {
                info!("   🔗 Connection Stats:");
                for (exchange, stat) in stats.iter() {
                    info!("      {}: {} connected, {} disconnected, {} total",
                          exchange, stat.connected, stat.disconnected, stat.total_connections);
                }
            }
        }

        // Sample recent trade data
        if trade_count > last_trade_count {
            let trades = TRADES.read();
            if let Some(latest_trade) = trades.iter().last() {
                info!("   🔥 Latest Trade: {} {} {:.8} @ {:.8} ({})",
                      latest_trade.exchange,
                      latest_trade.symbol,
                      latest_trade.quantity,
                      latest_trade.price,
                      latest_trade.asset_type);
            }
        }

        // Sample recent orderbook data
        if orderbook_count > last_orderbook_count {
            let orderbooks = ORDERBOOKS.read();
            if let Some(latest_book) = orderbooks.iter().last() {
                let best_bid = latest_book.bids.first().map(|(p, q)| (*p, *q));
                let best_ask = latest_book.asks.first().map(|(p, q)| (*p, *q));
                info!("   📚 Latest OrderBook: {} {} | Bid: {:?} | Ask: {:?}",
                      latest_book.exchange,
                      latest_book.symbol,
                      best_bid,
                      best_ask);
            }
        }

        last_trade_count = trade_count;
        last_orderbook_count = orderbook_count;

        if elapsed >= duration_secs {
            info!("⏰ Monitoring duration completed");
            break;
        }
    }

    // Final summary
    info!("📊 Final Summary:");
    info!("   📈 Total Trades: {}", TRADES.read().len());
    info!("   📖 Total Orderbooks: {}", ORDERBOOKS.read().len());

    let stats = CONNECTION_STATS.read();
    if !stats.is_empty() {
        info!("   🔗 Final Connection Stats:");
        for (exchange, stat) in stats.iter() {
            info!("      {}: {} connected, {} disconnected, {} total",
                  exchange, stat.connected, stat.disconnected, stat.total_connections);
        }
    }
}