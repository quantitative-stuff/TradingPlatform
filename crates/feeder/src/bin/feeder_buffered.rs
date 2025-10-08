use tokio::signal;
use anyhow::Result;
use std::path::PathBuf;
use std::env;
use std::fs::File;
use std::sync::Arc;
use serde_json::Value;
use tokio::sync::RwLock;

use feeder::core::{Feeder, SymbolMapper, CONNECTION_STATS, FileLogger, get_ordered_udp_sender, init_global_ordered_udp_sender, trigger_shutdown, init_feeder_logger};
use feeder::core::robust_connection::ExchangeConnectionLimits;
use feeder::core::{log_connection, log_performance};
use feeder::crypto::feed::{BinanceExchange, BybitExchange, UpbitExchange, CoinbaseExchange, OkxExchange, DeribitExchange, BithumbExchange};
use feeder::load_config::ExchangeConfig;
use feeder::connect_to_databse::{FeederLogger, QuestDBClient, QuestDBConfig};
use std::time::Duration;
use tracing::{info, warn, error};

// Buffered UDP sender - with sequence ordering and buffering
const MONITOR_ENDPOINT: &str = "239.255.42.99:9001";

#[derive(Clone)]
struct ExtendedExchangeConfig {
    pub base_config: ExchangeConfig,
    pub spot_symbols: Vec<String>,
    pub futures_symbols: Vec<String>,
    pub krw_symbols: Vec<String>,
}

impl ExtendedExchangeConfig {
    fn load_full_config(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let json: Value = serde_json::from_reader(file)?;

        let spot_symbols = json["subscribe_data"]["spot_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let futures_symbols = json["subscribe_data"]["futures_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let krw_symbols = json["subscribe_data"]["krw_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let base_config = ExchangeConfig::new(path)?;

        Ok(Self {
            base_config,
            spot_symbols,
            futures_symbols,
            krw_symbols,
        })
    }
}

#[derive(Clone)]
struct ConnectionInfo {
    exchange: String,
    symbols: Vec<String>,
}

async fn run_feeder_buffered() -> Result<()> {
    // No terminal display for production
    let terminal_display = false;

    // Initialize file logging only (no terminal output)
    let file_logger = FileLogger::new();
    if let Err(e) = file_logger.init() {
        error!("Failed to initialize file logger: {}", e);
        return Err(anyhow::anyhow!("Failed to initialize logging"));
    }

    // Get exchanges from environment variable
    // Format: EXCHANGES=binance,bybit,upbit,coinbase,okx,deribit,bithumb
    let exchanges_env = env::var("EXCHANGES")
        .unwrap_or_else(|_| "binance".to_string()); // Default to binance only

    // Get buffer size from environment (default 1000 packets)
    let buffer_size: usize = env::var("BUFFER_SIZE")
        .unwrap_or_else(|_| "1000".to_string())
        .parse()
        .unwrap_or(1000);

    // Get flush interval from environment (default 10ms)
    let flush_interval_ms: u64 = env::var("FLUSH_INTERVAL_MS")
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .unwrap_or(10);

    let selected_exchanges: Vec<&str> = exchanges_env
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if selected_exchanges.is_empty() {
        error!("No exchanges specified in EXCHANGES environment variable");
        return Err(anyhow::anyhow!("No exchanges specified"));
    }

    info!("Starting buffered feeder with exchanges: {:?}", selected_exchanges);
    info!("Buffer configuration: {} packets, {}ms flush interval", buffer_size, flush_interval_ms);

    let config_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
        .join("config");

    let symbol_mapper_path = config_dir.join("crypto/symbol_mapping.json");
    if !symbol_mapper_path.exists() {
        return Err(anyhow::anyhow!("symbol_mapping.json not found"));
    }

    let symbol_mapper: SymbolMapper = serde_json::from_reader(
        File::open(&symbol_mapper_path)?
    )?;
    let symbol_mapper = Arc::new(symbol_mapper);
    info!("Symbol mapper loaded successfully");

    // Initialize QuestDB for feeder logging
    let questdb_config = QuestDBConfig::default();
    match QuestDBClient::new(questdb_config.clone()).await {
        Ok(questdb_client) => {
            info!("✅ Connected to QuestDB for feeder logging");

            let questdb_for_logging = Arc::new(RwLock::new(questdb_client.clone()));
            let file_logger = FileLogger::new();
            if let Err(e) = file_logger.init_with_questdb(Some(questdb_for_logging)) {
                warn!("Failed to initialize file logger with QuestDB: {}", e);
            }

            // Initialize FeederLogger with QuestDB
            let feeder_logger = FeederLogger::new(
                questdb_client,
                5000
            ).await?;

            if let Err(e) = init_feeder_logger(feeder_logger).await {
                warn!("Failed to initialize global feeder logger: {}", e);
            } else {
                info!("✅ Feeder logger initialized");
            }
        },
        Err(e) => {
            warn!("⚠️ Failed to connect to QuestDB: {}. Using file logging only.", e);
        }
    }

    // Load exchange configurations
    let mut all_handles = Vec::new();

    for exchange_name in &selected_exchanges {
        match exchange_name {
            &"binance" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("crypto/binance_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting Binance with {} spot + {} futures symbols",
                          config.spot_symbols.len(), config.futures_symbols.len());

                    let handle = spawn_binance_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Binance config");
                }
            },
            &"bybit" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("crypto/bybit_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting Bybit with {} spot + {} futures symbols",
                          config.spot_symbols.len(), config.futures_symbols.len());

                    let handle = spawn_bybit_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Bybit config");
                }
            },
            &"upbit" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("crypto/upbit_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting Upbit with {} symbols",
                          config.base_config.subscribe_data.codes.len() + config.krw_symbols.len());

                    let handle = spawn_upbit_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Upbit config");
                }
            },
            &"coinbase" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("crypto/coinbase_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting Coinbase with {} spot symbols", config.spot_symbols.len());

                    let handle = spawn_coinbase_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Coinbase config");
                }
            },
            &"okx" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("crypto/okx_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting OKX with {} spot + {} futures symbols",
                          config.spot_symbols.len(), config.futures_symbols.len());

                    let handle = spawn_okx_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load OKX config");
                }
            },
            &"deribit" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("crypto/deribit_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting Deribit with {} futures symbols", config.futures_symbols.len());

                    let handle = spawn_deribit_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Deribit config");
                }
            },
            &"bithumb" => {
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join("bithumb_config_full.json").to_str().unwrap()
                ) {
                    info!("Starting Bithumb with {} spot symbols", config.krw_symbols.len());

                    let handle = spawn_bithumb_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Bithumb config");
                }
            },
            _ => {
                error!("Unknown exchange: {}", exchange_name);
            }
        }
    }

    if all_handles.is_empty() {
        error!("No exchanges started successfully");
        return Err(anyhow::anyhow!("No exchanges started"));
    }

    // Initialize BUFFERED UDP sender with ordering
    if let Err(e) = init_global_ordered_udp_sender("0.0.0.0:0", MONITOR_ENDPOINT, Some(100), Some(flush_interval_ms)).await {
        error!("Failed to initialize buffered UDP sender: {}", e);
        return Err(anyhow::anyhow!("Failed to initialize UDP sender"));
    }
    info!("✅ Buffered UDP sender initialized (with ordering, buffer: {}, flush: {}ms)", buffer_size, flush_interval_ms);

    // Send startup notification
    if let Some(sender) = get_ordered_udp_sender() {
        let _ = sender.send_stats("FEEDER_BUFFERED", selected_exchanges.len(), 1).await;
    }

    // Connection monitor with buffered sending
    let connection_monitor_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Some(sender) = get_ordered_udp_sender() {
                        let stats_to_send = {
                            let conn_stats = CONNECTION_STATS.read();
                            conn_stats.iter()
                                .filter(|(exchange, _)| exchange.contains('_'))
                                .map(|(exchange, stats)| {
                                    (
                                        exchange.clone(),
                                        stats.connected,
                                        stats.disconnected,
                                        stats.reconnect_count,
                                        stats.total_connections
                                    )
                                }).collect::<Vec<_>>()
                        };

                        for (exchange, connected, disconnected, reconnect_count, total) in stats_to_send {
                            let _ = sender.send_connection_status(
                                &exchange,
                                connected,
                                disconnected,
                                reconnect_count,
                                total
                            ).await;
                        }
                    }
                }
            }
        }
    });

    info!("🚀 Buffered feeder running with {} exchanges", all_handles.len());
    info!("📡 UDP multicast: {} (buffered sending with ordering)", MONITOR_ENDPOINT);
    info!("🔄 Buffer settings: {} packets, {}ms flush interval", buffer_size, flush_interval_ms);
    info!("📝 Logs: check feeder.log");

    // Wait for shutdown signal
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    info!("Received shutdown signal, stopping...");

    trigger_shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;

    connection_monitor_handle.abort();
    for handle in all_handles {
        handle.abort();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;
    info!("Buffered feeder stopped cleanly");

    Ok(())
}

async fn spawn_binance_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_binance = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "futures" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_binance.subscribe_data.codes = all_symbols;
        let mut exchange = BinanceExchange::new(config_for_binance, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Binance: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Binance: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Binance: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Binance start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Binance subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Binance connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn spawn_bybit_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_bybit = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "linear" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_bybit.subscribe_data.codes = all_symbols;
        let mut exchange = BybitExchange::new(config_for_bybit, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Bybit: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Bybit: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Bybit: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Bybit start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Bybit subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Bybit connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn spawn_upbit_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut complete_config = config.base_config.clone();
        let mut all_symbols = complete_config.subscribe_data.codes.clone();
        all_symbols.extend(config.krw_symbols.clone());
        complete_config.subscribe_data.codes = all_symbols;

        let mut exchange = UpbitExchange::new(complete_config, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Upbit: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Upbit: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Upbit: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Upbit start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Upbit subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Upbit connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn spawn_coinbase_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_coinbase = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "futures" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_coinbase.subscribe_data.codes = all_symbols;
        let mut exchange = CoinbaseExchange::new(config_for_coinbase, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Coinbase: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Coinbase: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Coinbase: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Coinbase start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Coinbase subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Coinbase connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn spawn_okx_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_okx = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "swap" | "futures" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_okx.subscribe_data.codes = all_symbols;
        let mut exchange = OkxExchange::new(config_for_okx, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("OKX: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("OKX: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("OKX: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("OKX start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("OKX subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("OKX connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn spawn_deribit_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_deribit = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "futures" | "options" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_deribit.subscribe_data.codes = all_symbols;
        let mut exchange = DeribitExchange::new(config_for_deribit, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Deribit: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Deribit: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Deribit: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Deribit start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Deribit subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Deribit connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn spawn_bithumb_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_bithumb = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.krw_symbols.clone()),
                _ => {},
            }
        }

        config_for_bithumb.subscribe_data.codes = all_symbols;
        let mut exchange = BithumbExchange::new(config_for_bithumb, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Bithumb: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Bithumb: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Bithumb: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Bithumb start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Bithumb subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Bithumb connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

#[tokio::main]
async fn main() {
    if let Err(e) = run_feeder_buffered().await {
        error!("Feeder buffered failed: {}", e);
        eprintln!("\n❌ CRITICAL ERROR: {}\nCheck logs for details.\n", e);
    }
}