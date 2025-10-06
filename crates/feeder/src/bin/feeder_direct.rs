use tokio::signal;
use anyhow::Result;
use std::path::PathBuf;
use std::env;
use std::fs::File;
use std::sync::Arc;
use serde_json::Value;
use tokio::sync::RwLock;

use feeder::core::{Feeder, SymbolMapper, CONNECTION_STATS, FileLogger, init_global_binary_udp_sender, get_binary_udp_sender, trigger_shutdown, init_feeder_logger};
use feeder::core::spawning::*;
use feeder::crypto::feed::{BinanceExchange, BybitExchange, UpbitExchange, CoinbaseExchange, OkxExchange, DeribitExchange, BithumbExchange};
use feeder::load_config::ExchangeConfig;
use feeder::connect_to_databse::{FeederLogger, QuestDBClient, QuestDBConfig};
use std::time::Duration;
use tracing::{info, warn, error};

// Direct UDP sender - no buffering, immediate sending
const MONITOR_ENDPOINT: &str = "239.255.42.99:9001";

use feeder::core::spawning::*;
async fn run_feeder_direct() -> Result<()> {
    // No terminal display for production

    // Initialize file logging only (no terminal output)
    let file_logger = FileLogger::new();
    if let Err(e) = file_logger.init() {
        eprintln!("Failed to initialize file logger: {}", e);
        return Err(anyhow::anyhow!("Failed to initialize logging"));
    }

    // Check if we should use limited configs (default is full configs)
    let use_limited_config = std::env::var("USE_LIMITED_CONFIG").is_ok();
    if use_limited_config {
        info!("Using LIMITED configuration files (USE_LIMITED_CONFIG is set)");
        println!("Using LIMITED configuration files with fewer symbols for testing");
    } else {
        info!("Using FULL configuration files (default, set USE_LIMITED_CONFIG=1 for limited configs)");
        println!("Using FULL configuration files");
    }

    // Read exchanges from config file
    let config_file = std::fs::File::open("config/crypto/crypto_exchanges.json")?;
    let config: serde_json::Value = serde_json::from_reader(config_file)?;
    let selected_exchanges: Vec<String> = config["exchanges"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    if selected_exchanges.is_empty() {
        error!("No exchanges specified in config/crypto/crypto_exchanges.json");
        return Err(anyhow::anyhow!("No exchanges specified"));
    }

    info!("Starting direct feeder with exchanges: {:?}", selected_exchanges);

    // Set up config directory for exchange configs
    let config_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
        .join("config")
        .join("crypto");

    // Load symbol mapper directly from config/crypto/symbol_mapping.json
    let symbol_mapper_path = "config/crypto/symbol_mapping.json";
    if !PathBuf::from(symbol_mapper_path).exists() {
        error!("Symbol mapping file not found: {}", symbol_mapper_path);
        return Err(anyhow::anyhow!("symbol_mapping.json not found at {}", symbol_mapper_path));
    }

    info!("Loading symbol mapper from: {}", symbol_mapper_path);
    let symbol_mapper: SymbolMapper = serde_json::from_reader(
        File::open(symbol_mapper_path)?
    )?;
    let symbol_mapper = Arc::new(symbol_mapper);
    info!("✅ Symbol mapper loaded successfully with {} exchanges", symbol_mapper.exchanges.len());

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

            // Force debug logging
            std::env::set_var("RUST_LOG", "debug");

            if let Err(e) = init_feeder_logger(feeder_logger).await {
                warn!("Failed to initialize global feeder logger: {}", e);
            } else {
                info!("✅ Feeder logger initialized with DEBUG level");
            }
        },
        Err(e) => {
            warn!("⚠️ Failed to connect to QuestDB: {}. Using file logging only.", e);
        }
    }

    // Load exchange configurations
    let mut all_handles = Vec::new();

    for exchange_name in &selected_exchanges {
        match exchange_name.as_str() {
            "binance" => {
                let config_file = if use_limited_config { "binance_config.json" } else { "binance_config_full.json" };
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join(config_file).to_str().unwrap()
                ) {
                    info!("Starting Binance with {} spot + {} futures symbols",
                          config.spot_symbols.len(), config.futures_symbols.len());

                    let handle = spawn_binance_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Binance config");
                }
            },
            "bybit" => {
                let config_file = if use_limited_config { "bybit_config.json" } else { "bybit_config_full.json" };
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join(config_file).to_str().unwrap()
                ) {
                    info!("Starting Bybit with {} spot + {} futures symbols",
                          config.spot_symbols.len(), config.futures_symbols.len());

                    let handle = spawn_bybit_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Bybit config");
                }
            },
            "upbit" => {
                let config_file = if use_limited_config { "upbit_config.json" } else { "upbit_config_full.json" };
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join(config_file).to_str().unwrap()
                ) {
                    info!("Starting Upbit with {} symbols", config.spot_symbols.len());

                    let handle = spawn_upbit_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Upbit config");
                }
            },
            "coinbase" => {
                let config_file = if use_limited_config { "coinbase_config.json" } else { "coinbase_config_full.json" };
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join(config_file).to_str().unwrap()
                ) {
                    info!("Starting Coinbase with {} spot symbols", config.spot_symbols.len());

                    let handle = spawn_coinbase_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Coinbase config");
                }
            },
            "okx" => {
                let config_file = if use_limited_config { "okx_config.json" } else { "okx_config_full.json" };
                let config_path = config_dir.join(config_file);
                println!("Loading OKX config from: {:?}", config_path);
                match ExtendedExchangeConfig::load_full_config(config_path.to_str().unwrap()) {
                    Ok(config) => {
                    info!("Starting OKX with {} spot + {} futures symbols",
                          config.spot_symbols.len(), config.futures_symbols.len());

                    let handle = spawn_okx_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                    },
                    Err(e) => {
                        error!("Failed to load OKX config from {:?}: {}", config_path, e);
                        println!("Failed to load OKX config: {}", e);
                    }
                }
            },
            "deribit" => {
                let config_file = if use_limited_config { "deribit_config.json" } else { "deribit_config_full.json" };
                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_dir.join(config_file).to_str().unwrap()
                ) {
                    info!("Starting Deribit with {} futures symbols", config.futures_symbols.len());

                    let handle = spawn_deribit_exchange(config, symbol_mapper.clone()).await;
                    all_handles.push(handle);
                } else {
                    error!("Failed to load Deribit config");
                }
            },
            "bithumb" => {
                let config_file = if use_limited_config { "bithumb_config.json" } else { "bithumb_config_full.json" };
                println!("Loading Bithumb config from: {}", config_file);
                match ExtendedExchangeConfig::load_full_config(
                    config_dir.join(config_file).to_str().unwrap()
                ) {
                    Ok(config) => {
                        println!("Bithumb config loaded - spot_symbols: {:?}", config.spot_symbols);
                        info!("Starting Bithumb with {} spot symbols", config.spot_symbols.len());

                        let handle = spawn_bithumb_exchange(config, symbol_mapper.clone()).await;
                        all_handles.push(handle);
                    }
                    Err(e) => {
                        error!("Failed to load Bithumb config: {}", e);
                        println!("Failed to load Bithumb config: {}", e);
                    }
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

    // Initialize BINARY UDP sender (using binary packet format from udp_packet.md)
    if let Err(e) = init_global_binary_udp_sender("0.0.0.0:0", MONITOR_ENDPOINT).await {
        error!("Failed to initialize binary UDP sender: {}", e);
        return Err(anyhow::anyhow!("Failed to initialize binary UDP sender"));
    }
    info!("✅ Binary UDP sender initialized (66-byte headers + binary payload)");

    // Send startup notification
    if let Some(sender) = get_binary_udp_sender() {
        let _ = sender.send_stats("FEEDER_DIRECT", selected_exchanges.len(), 1);
    }

    // Connection monitor
    let connection_monitor_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Some(sender) = get_binary_udp_sender() {
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
                            );
                        }
                    }
                }
            }
        }
    });

    info!("🚀 Direct feeder running with {} exchanges", all_handles.len());
    info!("📡 UDP multicast: {} (binary packet format, 90% smaller packets)", MONITOR_ENDPOINT);
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
    info!("Direct feeder stopped cleanly");

    Ok(())
}


#[tokio::main]
async fn main() {
    if let Err(e) = run_feeder_direct().await {
        eprintln!("Feeder direct failed: {}", e);
    }
}