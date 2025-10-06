use tokio::signal;
use anyhow::Result;
use std::path::PathBuf;
use std::env;
use std::fs::File;
use std::sync::Arc;
use std::collections::HashSet;
use serde_json::Value;
use tokio::sync::RwLock;

use feeder::core::{Feeder, SymbolMapper, CONNECTION_STATS, FileLogger, TerminalMonitor, init_global_binary_udp_sender, get_binary_udp_sender, trigger_shutdown, init_feeder_logger};
use feeder::core::robust_connection::ExchangeConnectionLimits;
use feeder::core::{log_connection, log_performance};
use feeder::crypto::feed::{BinanceExchange, BybitExchange, UpbitExchange, CoinbaseExchange, OkxExchange, DeribitExchange, BithumbExchange};
use feeder::load_config::ExchangeConfig;
use feeder::connect_to_databse::{FeederLogger, QuestDBClient, QuestDBConfig};
use std::time::Duration;
use tracing::{info, warn, error};

// Monitor endpoint configuration
const MONITOR_ENDPOINT: &str = "239.255.42.99:9001";




async fn run_feeder() -> Result<()> {
    // Check if terminal display is enabled (default: false for silent operation)
    let terminal_display = env::var("TERMINAL_DISPLAY")
        .unwrap_or_else(|_| "false".to_string()) == "true";
    
    // File logging will be initialized after QuestDB is connected
    
    // Start terminal monitor if enabled
    let mut terminal_monitor = TerminalMonitor::new(terminal_display);
    if let Err(e) = terminal_monitor.start() {
        warn!("Failed to start terminal monitor: {}", e);
    }
    
    // Interactive exchange selection
    println!("========================================");
    println!("       CRYPTO FEEDER - EXCHANGE SELECTION");
    println!("========================================");
    println!();
    println!("Available exchanges:");
    println!("  1. Binance (Spot & Futures)");
    println!("  2. Bybit (Spot & Linear)");
    println!("  3. Upbit (KRW pairs)");
    println!("  4. Coinbase (USD pairs)");
    println!("  5. OKX (Spot & Swaps)");
    println!("  6. Deribit (Derivatives only)");
    println!("  7. Bithumb (KRW pairs)");
    println!("  8. All exchanges");
    println!("  9. Standard set (Binance, Bybit, Upbit)");
    println!("  0. Exit");
    println!();
    print!("Select exchanges (comma-separated, e.g., 1,2,4): ");
    
    use std::io::{self, Write};
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let selections: Vec<&str> = input.trim().split(',').collect();
    
    let mut selected_exchanges = HashSet::new();
    
    for selection in selections {
        match selection.trim() {
            "1" => { selected_exchanges.insert("binance"); },
            "2" => { selected_exchanges.insert("bybit"); },
            "3" => { selected_exchanges.insert("upbit"); },
            "4" => { 
                info!("User selected Coinbase (option 4)");
                selected_exchanges.insert("coinbase"); 
            },
            "5" => { selected_exchanges.insert("okx"); },
            "6" => { selected_exchanges.insert("deribit"); },
            "7" => { selected_exchanges.insert("bithumb"); },
            "8" => {
                selected_exchanges.insert("binance");
                selected_exchanges.insert("bybit");
                selected_exchanges.insert("upbit");
                selected_exchanges.insert("coinbase");
                selected_exchanges.insert("okx");
                selected_exchanges.insert("deribit");
                selected_exchanges.insert("bithumb");
            },
            "9" => {
                selected_exchanges.insert("binance");
                selected_exchanges.insert("bybit");
                selected_exchanges.insert("upbit");
            },
            "0" => {
                println!("Exiting...");
                return Ok(());
            },
            _ => {
                println!("Invalid selection: {}", selection.trim());
            }
        }
    }
    
    if selected_exchanges.is_empty() {
        println!("No exchanges selected. Exiting...");
        error!("No exchanges selected. Exiting...");
        return Ok(());
    }
    
    println!();
    println!("Starting feeder with selected exchanges: {:?}", selected_exchanges);
    info!("Starting feeder with selected exchanges: {:?}", selected_exchanges);
    println!();
    
    let config_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
        .join("config");
    
    let symbol_mapper_path = config_dir.join("crypto/symbol_mapping.json");
    if !symbol_mapper_path.exists() {
        return Err(anyhow::anyhow!("symbol_mapping.json not found"));
    }
    
    let symbol_mapper: SymbolMapper = serde_json::from_reader(
        File::open(&symbol_mapper_path)?
    )?;
    
    // Symbol mapper loaded
    
    let symbol_mapper = Arc::new(symbol_mapper);
    info!("Symbol mapper loaded successfully");
    
    // Initialize QuestDB for feeder logging
    info!("Initializing QuestDB for feeder logging...");
    let mut questdb_config = QuestDBConfig::default();
    
    // Allow override from environment variables
    if let Ok(host) = env::var("QUESTDB_HOST") {
        questdb_config.host = host.clone();
        info!("Using QuestDB host from env: {}", host);
    }
    if let Ok(port) = env::var("QUESTDB_ILP_PORT") {
        if let Ok(port_num) = port.parse::<u16>() {
            questdb_config.ilp_port = port_num;
            info!("Using QuestDB ILP port from env: {}", port_num);
        }
    }
    match QuestDBClient::new(questdb_config.clone()).await {
        Ok(questdb_client) => {
            info!("✅ Connected to QuestDB at {}:{} for feeder logging", questdb_config.host, questdb_config.ilp_port);
            
            // Initialize file logging with QuestDB integration
            let enable_file_logging = env::var("FILE_LOGGING")
                .unwrap_or_else(|_| "true".to_string()) == "true";
            
            let questdb_for_logging = Arc::new(RwLock::new(questdb_client.clone()));
            
            if enable_file_logging {
                let file_logger = FileLogger::new();
                if let Err(e) = file_logger.init_with_questdb(Some(questdb_for_logging)) {
                    eprintln!("Failed to initialize file logger with QuestDB: {}", e);
                    return Err(anyhow::anyhow!("Failed to initialize logging"));
                }
                info!("✅ File logging enabled with QuestDB integration");
            } else {
                // Initialize QuestDB-only logging without file logging
                let file_logger = FileLogger::new();
                if let Err(e) = file_logger.init_with_questdb(Some(questdb_for_logging)) {
                    eprintln!("Failed to initialize QuestDB logging: {}", e);
                    return Err(anyhow::anyhow!("Failed to initialize logging"));
                }
                info!("📝 File logging disabled, QuestDB logging enabled");
            }
            
            // Initialize FeederLogger with QuestDB
            let feeder_logger = FeederLogger::new(
                questdb_client,
                5000 // Flush every 5 seconds
            ).await?;

            // Initialize global feeder logger
            if let Err(e) = init_feeder_logger(feeder_logger).await {
                warn!("Failed to initialize global feeder logger: {}", e);
            } else {
                info!("✅ Feeder logger initialized with QuestDB backend");
            }
        },
        Err(e) => {
            warn!("⚠️ Failed to connect to QuestDB: {}. Using file logging only.", e);
            
            // Initialize file logging without QuestDB
            let enable_file_logging = env::var("FILE_LOGGING")
                .unwrap_or_else(|_| "true".to_string()) == "true";
            
            if enable_file_logging {
                let file_logger = FileLogger::new();
                if let Err(e) = file_logger.init() {
                    eprintln!("Failed to initialize file logger: {}", e);
                    return Err(anyhow::anyhow!("Failed to initialize logging"));
                }
                info!("✅ File logging enabled (QuestDB unavailable)");
            } else {
                warn!("⚠️ Both QuestDB and file logging are disabled - no logging available!");
            }
        }
    }
    
    // Load exchange configurations
    
    
    // Load all exchange configurations
    let binance_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/binance_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded Binance config with {} spot and {} futures symbols",
                config.spot_symbols.len(), config.futures_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load Binance full config: {}", e);
            None
        }
    };
    
    let bybit_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/bybit_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded Bybit config with {} spot and {} futures symbols",
                config.spot_symbols.len(), config.futures_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load Bybit full config: {}", e);
            None
        }
    };
    
    let upbit_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/upbit_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded Upbit config with {} KRW symbols",
                config.krw_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load Upbit full config: {}", e);
            None
        }
    };
    
    // Load new exchange configurations with full symbols
    let coinbase_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/coinbase_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded Coinbase config with {} spot symbols", config.spot_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load Coinbase full config: {}", e);
            None
        }
    };
    
    let okx_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/okx_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded OKX config with {} spot and {} futures symbols", 
                config.spot_symbols.len(), config.futures_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load OKX full config: {}", e);
            None
        }
    };
    
    let deribit_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/deribit_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded Deribit config with {} futures symbols", config.futures_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load Deribit full config: {}", e);
            None
        }
    };
    
    let bithumb_config = match ExtendedExchangeConfig::load_full_config(
        config_dir.join("crypto/bithumb_config_full.json").to_str().unwrap()
    ) {
        Ok(config) => {
            info!("Loaded Bithumb config with {} spot symbols", config.spot_symbols.len());
            Some(config)
        },
        Err(e) => {
            warn!("Failed to load Bithumb full config: {}", e);
            None
        }
    };
    
    // Check if at least one selected exchange has a config
    let has_config = (selected_exchanges.contains("binance") && binance_config.is_some()) ||
                     (selected_exchanges.contains("bybit") && bybit_config.is_some()) ||
                     (selected_exchanges.contains("upbit") && upbit_config.is_some()) ||
                     (selected_exchanges.contains("coinbase") && coinbase_config.is_some()) ||
                     (selected_exchanges.contains("okx") && okx_config.is_some()) ||
                     (selected_exchanges.contains("deribit") && deribit_config.is_some()) ||
                     (selected_exchanges.contains("bithumb") && bithumb_config.is_some());
    
    if !has_config {
        error!("ERROR: No configs could be loaded for selected exchanges!");
        return Err(anyhow::anyhow!("No configs could be loaded for selected exchanges"));
    }
    
    let mut all_handles = Vec::new();
    
    // Start selected exchanges directly (no orchestrator)
    if selected_exchanges.contains("binance") {
        if let Some(config) = binance_config {
            info!("Starting Binance exchange directly (no orchestrator)");
            
            // Create Binance exchange with all asset types and symbols
            let mut config_for_binance = config.base_config.clone();
            
            // Combine all symbols based on asset types
            let mut all_symbols = Vec::new();
            let mut total_spot = 0;
            let mut total_futures = 0;
            
            for asset_type in &config.base_config.feed_config.asset_type {
                match asset_type.as_str() {
                    "spot" => {
                        all_symbols.extend(config.spot_symbols.clone());
                        total_spot = config.spot_symbols.len();
                    }
                    "futures" => {
                        all_symbols.extend(config.futures_symbols.clone());  
                        total_futures = config.futures_symbols.len();
                    }
                    _ => warn!("Binance: Unknown asset type: {}", asset_type),
                }
            }
            
            config_for_binance.subscribe_data.codes = all_symbols.clone();
            // Keep original asset types from config
            
            // Calculate and log connection computation
            let limits = ExchangeConnectionLimits::for_exchange("binance");
            let total_symbols = all_symbols.len();
            let symbols_per_connection = limits.max_symbols_per_connection;
            let connections_needed = (total_symbols + symbols_per_connection - 1) / symbols_per_connection;
            info!("Binance: {} spot + {} futures = {} total symbols ÷ {} symbols/connection = {} connections needed", 
                  total_spot, total_futures, total_symbols, symbols_per_connection, connections_needed);
            
            let mut exchange = BinanceExchange::new(config_for_binance, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                            Ok(_) => {
                                info!("Binance: Connected successfully");
                                match exchange.subscribe().await {
                                    Ok(_) => {
                                        info!("Binance: Subscribed successfully");
                                        match exchange.start().await {
                                            Ok(_) => {
                                                // start() returns when connections are lost
                                                warn!("Binance: All connections lost, reconnecting...");
                                                tokio::time::sleep(Duration::from_secs(5)).await;
                                                break; // Break inner loop to reconnect
                                            }
                                            Err(e) => {
                                                error!("Binance: Start error: {}. Retrying in 10s...", e);
                                                tokio::time::sleep(Duration::from_secs(10)).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Binance: Subscribe error: {}. Retrying in 10s...", e);
                                        tokio::time::sleep(Duration::from_secs(10)).await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Binance: Connect error: {}. Retrying in 10s...", e);
                                tokio::time::sleep(Duration::from_secs(10)).await;
                            }
                        }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "Binance".to_string(),
                symbols: config.spot_symbols.clone(),
            }));
        }
    }
    
    if selected_exchanges.contains("bybit") {
        if let Some(config) = bybit_config {
            info!("Starting Bybit exchange directly (no orchestrator)");
            info!("⚠️  Bybit connections will be throttled to avoid 403 errors");
            
            // Create Bybit exchange with all asset types and symbols
            let mut config_for_bybit = config.base_config.clone();
            
            // Combine all symbols based on asset types
            let mut all_symbols = Vec::new();
            let mut total_spot = 0;
            let mut total_linear = 0;
            
            for asset_type in &config.base_config.feed_config.asset_type {
                match asset_type.as_str() {
                    "spot" => {
                        all_symbols.extend(config.spot_symbols.clone());
                        total_spot = config.spot_symbols.len();
                    }
                    "linear" => {
                        all_symbols.extend(config.futures_symbols.clone());
                        total_linear = config.futures_symbols.len();
                    }
                    _ => warn!("Bybit: Unknown asset type: {}", asset_type),
                }
            }
            
            config_for_bybit.subscribe_data.codes = all_symbols.clone();
            // Keep original asset types from config
            
            // Calculate and log connection computation
            let limits = ExchangeConnectionLimits::for_exchange("bybit");
            let total_symbols = all_symbols.len();
            let symbols_per_connection = limits.max_symbols_per_connection;
            let connections_needed = (total_symbols + symbols_per_connection - 1) / symbols_per_connection;
            info!("Bybit: {} spot + {} linear = {} total symbols ÷ {} symbols/connection = {} connections needed", 
                  total_spot, total_linear, total_symbols, symbols_per_connection, connections_needed);
            
            let mut exchange = BybitExchange::new(config_for_bybit, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                            Ok(_) => {
                                info!("Bybit: Connected successfully");
                                match exchange.subscribe().await {
                                    Ok(_) => {
                                        info!("Bybit: Subscribed successfully");
                                        match exchange.start().await {
                                            Ok(_) => {
                                                // start() returns when connections are lost
                                                warn!("Bybit: All connections lost, reconnecting...");
                                                tokio::time::sleep(Duration::from_secs(5)).await;
                                                break; // Break inner loop to reconnect
                                            }
                                            Err(e) => {
                                                error!("Bybit: Start error: {}. Retrying in 10s...", e);
                                                tokio::time::sleep(Duration::from_secs(10)).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Bybit: Subscribe error: {}. Retrying in 10s...", e);
                                        tokio::time::sleep(Duration::from_secs(10)).await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Bybit: Connect error: {}. Retrying in 10s...", e);
                                tokio::time::sleep(Duration::from_secs(10)).await;
                            }
                        }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "Bybit".to_string(),
                symbols: config.spot_symbols.clone(),
            }));
        }
    }
    
    if selected_exchanges.contains("upbit") {
        if let Some(config) = upbit_config {
            info!("Starting Upbit exchange directly (no orchestrator)");
            info!("⚠️  Upbit uses strict rate limiting - connections will be delayed");
            
            // Create complete config with all symbols (codes + krw_symbols)
            let mut complete_config = config.base_config.clone();
            let mut all_symbols = complete_config.subscribe_data.codes.clone();
            all_symbols.extend(config.krw_symbols.clone());
            complete_config.subscribe_data.codes = all_symbols;
            
            info!("Upbit: Combined {} USDT symbols + {} KRW symbols = {} total symbols", 
                  config.base_config.subscribe_data.codes.len(), 
                  config.krw_symbols.len(),
                  complete_config.subscribe_data.codes.len());
            
            // Create Upbit exchange with complete symbol list
            let mut exchange = UpbitExchange::new(complete_config, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                            Ok(_) => {
                                info!("Upbit: Connected successfully");
                            match exchange.subscribe().await {
                                Ok(_) => {
                                    info!("Upbit: Subscribed successfully");
                                    match exchange.start().await {
                                        Ok(_) => {
                                            // start() returns when connections are lost
                                            warn!("Upbit: All connections lost, reconnecting...");
                                            tokio::time::sleep(Duration::from_secs(5)).await;
                                            break; // Break inner loop to reconnect
                                        }
                                        Err(e) => {
                                            error!("Upbit: Start error: {}. Retrying in 10s...", e);
                                            tokio::time::sleep(Duration::from_secs(10)).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Upbit: Subscribe error: {}. Retrying in 10s...", e);
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Upbit: Connect error: {}. Retrying in 10s...", e);
                            tokio::time::sleep(Duration::from_secs(10)).await;
                        }
                    }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "Upbit".to_string(),
                symbols: config.base_config.subscribe_data.codes.clone(),
            }));
        }
    }
    
    if selected_exchanges.contains("coinbase") {
        if let Some(config) = coinbase_config {
            info!("Starting Coinbase exchange directly (no orchestrator)");
            info!("Note: Full data access requires authentication");
            
            // Create Coinbase exchange with all asset types and symbols
            let mut config_for_coinbase = config.base_config.clone();
            
            // Combine all symbols based on asset types (Coinbase is typically spot-only)
            let mut all_symbols = Vec::new();
            let mut total_spot = 0;
            let mut total_futures = 0;
            
            for asset_type in &config.base_config.feed_config.asset_type {
                match asset_type.as_str() {
                    "spot" => {
                        all_symbols.extend(config.spot_symbols.clone());
                        total_spot = config.spot_symbols.len();
                    }
                    "futures" => {
                        all_symbols.extend(config.futures_symbols.clone());
                        total_futures = config.futures_symbols.len();
                    }
                    _ => warn!("Coinbase: Unknown asset type: {}", asset_type),
                }
            }
            
            config_for_coinbase.subscribe_data.codes = all_symbols.clone();
            // Keep original asset types from config
            
            // Calculate and log connection computation
            let limits = ExchangeConnectionLimits::for_exchange("coinbase");
            let total_symbols = all_symbols.len();
            let symbols_per_connection = limits.max_symbols_per_connection;
            let connections_needed = (total_symbols + symbols_per_connection - 1) / symbols_per_connection;
            info!("Coinbase: {} spot + {} futures = {} total symbols ÷ {} symbols/connection = {} connections needed", 
                  total_spot, total_futures, total_symbols, symbols_per_connection, connections_needed);
            
            let mut exchange = CoinbaseExchange::new(config_for_coinbase, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                        Ok(_) => {
                            info!("Coinbase: Connected successfully");
                            match exchange.subscribe().await {
                                Ok(_) => {
                                    info!("Coinbase: Subscribed successfully");
                                    match exchange.start().await {
                                        Ok(_) => {
                                            // start() returns when connections are lost
                                            warn!("Coinbase: All connections lost, reconnecting...");
                                            tokio::time::sleep(Duration::from_secs(5)).await;
                                            break; // Break inner loop to reconnect
                                        }
                                        Err(e) => {
                                            error!("Coinbase: Start error: {}. Retrying in 10s...", e);
                                            tokio::time::sleep(Duration::from_secs(10)).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Coinbase: Subscribe error: {}. Retrying in 10s...", e);
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Coinbase: Connect error: {}. Retrying in 10s...", e);
                            tokio::time::sleep(Duration::from_secs(10)).await;
                        }
                    }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "Coinbase".to_string(),
                symbols: config.spot_symbols.clone(),
            }));
        } else {
            error!("Coinbase selected but config is None!");
        }
    }
    
    if selected_exchanges.contains("okx") {
        if let Some(config) = okx_config {
            info!("Starting OKX exchange directly (no orchestrator)");
            
            // Create OKX exchange with all asset types and symbols
            let mut config_for_okx = config.base_config.clone();
            
            // Combine all symbols based on asset types
            let mut all_symbols = Vec::new();
            let mut total_spot = 0;
            let mut total_futures = 0;
            
            for asset_type in &config.base_config.feed_config.asset_type {
                match asset_type.as_str() {
                    "spot" => {
                        all_symbols.extend(config.spot_symbols.clone());
                        total_spot = config.spot_symbols.len();
                    }
                    "swap" => {
                        // futures_symbols already contains both perpetual and futures from config parsing
                        all_symbols.extend(config.futures_symbols.clone());
                        total_futures = config.futures_symbols.len();
                    }
                    "futures" => {
                        // futures_symbols already contains both perpetual and futures from config parsing
                        if !config.base_config.feed_config.asset_type.contains(&"swap".to_string()) {
                            all_symbols.extend(config.futures_symbols.clone());
                            total_futures = config.futures_symbols.len();
                        }
                    }
                    _ => warn!("OKX: Unknown asset type: {}", asset_type),
                }
            }
            
            config_for_okx.subscribe_data.codes = all_symbols.clone();
            // Keep original asset types from config
            
            // Calculate and log connection computation
            let limits = ExchangeConnectionLimits::for_exchange("okx");
            let total_symbols = all_symbols.len();
            let symbols_per_connection = limits.max_symbols_per_connection;
            let connections_needed = (total_symbols + symbols_per_connection - 1) / symbols_per_connection;
            info!("OKX: {} spot + {} futures = {} total symbols ÷ {} symbols/connection = {} connections needed", 
                  total_spot, total_futures, total_symbols, symbols_per_connection, connections_needed);
            
            let mut exchange = OkxExchange::new(config_for_okx, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                            Ok(_) => {
                                info!("OKX: Connected successfully");
                                match exchange.subscribe().await {
                                    Ok(_) => {
                                        info!("OKX: Subscribed successfully");
                                        match exchange.start().await {
                                            Ok(_) => {
                                                // start() returns when connections are lost
                                                warn!("OKX: All connections lost, reconnecting...");
                                                tokio::time::sleep(Duration::from_secs(5)).await;
                                                break; // Break inner loop to reconnect
                                            }
                                            Err(e) => {
                                                error!("OKX: Start error: {}. Retrying in 10s...", e);
                                                tokio::time::sleep(Duration::from_secs(10)).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("OKX: Subscribe error: {}. Retrying in 10s...", e);
                                        tokio::time::sleep(Duration::from_secs(10)).await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("OKX: Connect error: {}. Retrying in 10s...", e);
                                tokio::time::sleep(Duration::from_secs(10)).await;
                            }
                        }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "OKX".to_string(),
                symbols: config.spot_symbols.clone(),
            }));
        }
    }
    
    if selected_exchanges.contains("deribit") {
        if let Some(config) = deribit_config {
            info!("Starting Deribit exchange directly (no orchestrator - derivatives only)");
            
            // Create Deribit exchange with all asset types and symbols 
            let mut config_for_deribit = config.base_config.clone();
            
            // Combine all symbols based on asset types (Deribit is derivatives-only)
            let mut all_symbols = Vec::new();
            let mut total_spot = 0;
            let mut total_futures = 0;
            
            for asset_type in &config.base_config.feed_config.asset_type {
                match asset_type.as_str() {
                    "spot" => {
                        all_symbols.extend(config.spot_symbols.clone());
                        total_spot = config.spot_symbols.len();
                    }
                    "futures" | "options" => {
                        all_symbols.extend(config.futures_symbols.clone());
                        total_futures = config.futures_symbols.len();
                    }
                    _ => warn!("Deribit: Unknown asset type: {}", asset_type),
                }
            }
            
            config_for_deribit.subscribe_data.codes = all_symbols.clone();
            // Keep original asset types from config
            
            // Calculate and log connection computation
            let limits = ExchangeConnectionLimits::for_exchange("deribit");
            let total_symbols = all_symbols.len();
            let symbols_per_connection = limits.max_symbols_per_connection;
            let connections_needed = (total_symbols + symbols_per_connection - 1) / symbols_per_connection;
            info!("Deribit: {} spot + {} futures = {} total symbols ÷ {} symbols/connection = {} connections needed", 
                  total_spot, total_futures, total_symbols, symbols_per_connection, connections_needed);
            
            let mut exchange = DeribitExchange::new(config_for_deribit, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                        Ok(_) => {
                            info!("Deribit: Connected successfully");
                            match exchange.subscribe().await {
                                Ok(_) => {
                                    info!("Deribit: Subscribed successfully");
                                    match exchange.start().await {
                                        Ok(_) => {
                                            // start() returns when connections are lost
                                            warn!("Deribit: All connections lost, reconnecting...");
                                            tokio::time::sleep(Duration::from_secs(5)).await;
                                            break; // Break inner loop to reconnect
                                        }
                                        Err(e) => {
                                            error!("Deribit: Start error: {}. Retrying in 10s...", e);
                                            tokio::time::sleep(Duration::from_secs(10)).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Deribit: Subscribe error: {}. Retrying in 10s...", e);
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Deribit: Connect error: {}. Retrying in 10s...", e);
                            tokio::time::sleep(Duration::from_secs(10)).await;
                        }
                    }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "Deribit".to_string(),
                symbols: config.futures_symbols.clone(),
            }));
        }
    }
    
    if selected_exchanges.contains("bithumb") {
        if let Some(config) = bithumb_config {
            info!("Starting Bithumb exchange directly (no orchestrator - KRW pairs)");
            
            // Create Bithumb exchange with all asset types and symbols
            let mut config_for_bithumb = config.base_config.clone();
            
            // Combine all symbols based on asset types (Bithumb uses KRW pairs)
            let mut all_symbols = Vec::new();
            let mut total_spot = 0;
            let mut total_futures = 0;
            
            for asset_type in &config.base_config.feed_config.asset_type {
                match asset_type.as_str() {
                    "spot" => {
                        // For Bithumb, spot symbols are in krw_symbols
                        all_symbols.extend(config.krw_symbols.clone());
                        total_spot = config.krw_symbols.len();
                    }
                    "futures" => {
                        all_symbols.extend(config.futures_symbols.clone());
                        total_futures = config.futures_symbols.len();
                    }
                    _ => warn!("Bithumb: Unknown asset type: {}", asset_type),
                }
            }
            
            config_for_bithumb.subscribe_data.codes = all_symbols.clone();
            // Keep original asset types from config
            
            // Calculate and log connection computation
            let limits = ExchangeConnectionLimits::for_exchange("bithumb");
            let total_symbols = all_symbols.len();
            let symbols_per_connection = limits.max_symbols_per_connection;
            let connections_needed = (total_symbols + symbols_per_connection - 1) / symbols_per_connection;
            info!("Bithumb: {} spot + {} futures = {} total symbols ÷ {} symbols/connection = {} connections needed", 
                  total_spot, total_futures, total_symbols, symbols_per_connection, connections_needed);
            
            let mut exchange = BithumbExchange::new(config_for_bithumb, symbol_mapper.clone());
            
            let handle = tokio::spawn(async move {
                loop {
                    // Outer loop for reconnection when all connections are lost
                    loop {
                        match exchange.connect().await {
                        Ok(_) => {
                            info!("Bithumb: Connected successfully");
                            match exchange.subscribe().await {
                                Ok(_) => {
                                    info!("Bithumb: Subscribed successfully");
                                    match exchange.start().await {
                                        Ok(_) => {
                                            // start() returns when connections are lost
                                            warn!("Bithumb: All connections lost, reconnecting...");
                                            tokio::time::sleep(Duration::from_secs(5)).await;
                                            break; // Break inner loop to reconnect
                                        }
                                        Err(e) => {
                                            error!("Bithumb: Start error: {}. Retrying in 10s...", e);
                                            tokio::time::sleep(Duration::from_secs(10)).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Bithumb: Subscribe error: {}. Retrying in 10s...", e);
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Bithumb: Connect error: {}. Retrying in 10s...", e);
                            tokio::time::sleep(Duration::from_secs(10)).await;
                        }
                    }
                    }
                }
            });
            
            all_handles.push((handle, ConnectionInfo {
                exchange: "Bithumb".to_string(),
                symbols: config.krw_symbols.clone(),
            }));
        }
    }
    
    // Clean up any legacy CONNECTION_STATS entries without asset type
    {
        let mut stats = CONNECTION_STATS.write();
        let legacy_keys: Vec<String> = stats.keys()
            .filter(|k| !k.contains('_'))
            .cloned()
            .collect();
        for key in legacy_keys {
            info!("Removing legacy CONNECTION_STATS entry: {}", key);
            stats.remove(&key);
        }
    }
    
    // Initialize optimized global UDP sender with channel-based architecture
    if let Err(e) = init_global_binary_udp_sender("0.0.0.0:0", MONITOR_ENDPOINT).await {
        error!("Failed to initialize optimized UDP sender: {}", e);
        return Err(anyhow::anyhow!("Failed to initialize optimized UDP sender"));
    }
    info!("Optimized UDP sender initialized - using channel-based architecture for high performance");
    
    // Send test packet (optimized sender methods are synchronous)
    if let Some(sender) = market_feeder::core::get_binary_udp_sender() {
        let _ = sender.send_stats("TEST", 0, 0);
    }
    
    // Connection status monitor - sends connection status updates periodically
    let connection_monitor_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut bybit_health_check = tokio::time::interval(Duration::from_secs(30)); // Check Bybit health every 30 seconds
        let mut metrics_interval = tokio::time::interval(Duration::from_secs(60)); // Log performance metrics every minute
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Send connection statistics using optimized sender
                    if let Some(sender) = market_feeder::core::get_binary_udp_sender() {
                        // Collect stats first to avoid holding lock
                        let stats_to_send = {
                            let conn_stats = CONNECTION_STATS.read();
                            conn_stats.iter()
                                // Filter out legacy entries without asset type (no underscore)
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
                        
                        // Now send without holding the lock (no await needed!)
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
                _ = bybit_health_check.tick() => {
                    // Check Bybit connection health
                    let bybit_stats = {
                        let conn_stats = CONNECTION_STATS.read();
                        conn_stats.iter()
                            .filter(|(exchange, _)| exchange.starts_with("Bybit_"))
                            .map(|(exchange, stats)| (exchange.clone(), stats.connected, stats.disconnected))
                            .collect::<Vec<_>>()
                    };
                    
                    for (exchange, connected, disconnected) in bybit_stats {
                        if connected == 0 && disconnected > 0 {
                            warn!("[{}] All connections are disconnected (connected: {}, disconnected: {})", 
                                exchange, connected, disconnected);
                        } else if disconnected > connected * 2 {
                            warn!("[{}] High disconnection rate detected (connected: {}, disconnected: {})", 
                                exchange, connected, disconnected);
                        }
                    }
                }
                _ = metrics_interval.tick() => {
                    // Log performance metrics to QuestDB
                    let stats = {
                        let conn_stats = CONNECTION_STATS.read();
                        conn_stats.iter()
                            .filter(|(exchange, _)| exchange.contains('_'))
                            .map(|(exchange, stats)| {
                                (
                                    exchange.clone(),
                                    stats.connected as u32,
                                    stats.total_connections as u64
                                )
                            }).collect::<Vec<_>>()
                    };
                    
                    // Log metrics for each exchange
                    for (exchange, active_connections, total_messages) in stats {
                        let metrics = market_feeder::connect_to_databse::PerformanceMetrics {
                            timestamp: chrono::Utc::now(),
                            exchange: exchange.clone(),
                            active_connections,
                            messages_per_second: 0.0, // Would need to calculate from message counts
                            avg_latency_ms: 0.0, // Would need to track latency
                            memory_usage_mb: 0.0, // Will be filled by collect_performance_metrics
                            cpu_usage_percent: 0.0, // Will be filled by collect_performance_metrics
                            dropped_messages: 0,
                            queue_depth: 0,
                        };
                        
                        log_performance(metrics).await;
                    }
                    
                    info!("Logged performance metrics to QuestDB");
                }
            }
        }
    });
    
    // Keep handles for potential future use
    let connection_handles = Arc::new(all_handles);
    
    // Calculate actual WebSocket connection count
    let mut total_websocket_connections = 0;
    for (_, conn_info) in connection_handles.iter() {
        match conn_info.exchange.as_str() {
            "Upbit" => {
                // Upbit: 5 symbols per connection
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection
                total_websocket_connections += connections;
                info!("Upbit: {} symbols -> {} WebSocket connections (5 symbols/conn)", symbols_count, connections);
            },
            "Binance" => {
                // Binance: 5 symbols per connection (with both trade and orderbook streams)
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection  
                total_websocket_connections += connections;
                info!("Binance: {} symbols -> {} WebSocket connections (5 symbols/conn)", symbols_count, connections);
            },
            "Bybit" => {
                // Bybit: 5 symbols per connection
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection
                total_websocket_connections += connections;
                info!("Bybit: {} symbols -> {} WebSocket connections (5 symbols/conn)", symbols_count, connections);
            },
            "Coinbase" => {
                // Coinbase: 5 symbols per connection
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection
                total_websocket_connections += connections;
                info!("Coinbase: {} symbols -> {} WebSocket connections", symbols_count, connections);
            },
            "OKX" => {
                // OKX: 5 symbols per connection
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection
                total_websocket_connections += connections;
                info!("OKX: {} symbols -> {} WebSocket connections (5 symbols/conn)", symbols_count, connections);
            },
            "Deribit" => {
                // Deribit: 5 symbols per connection
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection
                total_websocket_connections += connections;
                info!("Deribit: {} symbols -> {} WebSocket connections (5 symbols/conn)", symbols_count, connections);
            },
            "Bithumb" => {
                // Bithumb: 5 symbols per connection
                let symbols_count = conn_info.symbols.len();
                let connections = (symbols_count + 4) / 5; // 5 symbols per connection
                total_websocket_connections += connections;
                info!("Bithumb: {} symbols -> {} WebSocket connections (5 symbols/conn)", symbols_count, connections);
            },
            _ => {
                info!("Unknown exchange: {}", conn_info.exchange);
                total_websocket_connections += 1; // Default to 1 connection
            }
        }
    }
    
    info!("Total exchange tasks created: {}", connection_handles.len());
    info!("Total WebSocket connections expected: {}", total_websocket_connections);
    println!("✅ Feeder is now running with {} exchange tasks creating {} WebSocket connections", connection_handles.len(), total_websocket_connections);
    println!("📊 Monitor data on UDP ports: crypto=9001, stock=9002");
    println!("🔍 Check feeder.log for detailed logs");
    println!("⏹️  Press Ctrl+C to stop the feeder");
    println!();
    
    // Wait for Ctrl+C to shutdown
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    info!("Received shutdown signal, stopping...");
    
    // Trigger global shutdown signal first
    trigger_shutdown();
    
    // Give tasks a moment to see the shutdown signal
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Then abort ALL spawned tasks
    connection_monitor_handle.abort();
    
    // Abort all exchange connection handles
    for (handle, _info) in connection_handles.iter() {
        handle.abort();
    }
    
    // Give tasks a final moment to clean up
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    info!("All tasks aborted, shutting down cleanly");
    
    // Silent stop
    Ok(())
}

fn main() {
    // Silent operation - no panic output
    
    // Build multi-threaded tokio runtime
    
    // Build multi-threaded runtime for handling multiple exchanges
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)  // Use 8 worker threads for concurrent processing
        .enable_all()
        .build()
        .expect("Failed to create runtime");
    
    // Start async main
    
    // Block on the main thread
    let result = runtime.block_on(async {
        // Run the feeder silently
        run_feeder().await
    });
    
    // Silent error handling
    let _ = result;
}

// Helper function to load simple config files (with codes array)  
fn load_simple_config(path: &str) -> anyhow::Result<ExtendedExchangeConfig> {
    let base_config = ExchangeConfig::new(path)?;
    
    // Read the JSON to get the symbols from "codes" array
    let file = File::open(path)?;
    let json: Value = serde_json::from_reader(file)?;
    
    let codes = json["subscribe_data"]["codes"]
        .as_array()
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<String>>())
        .unwrap_or_default();
    
    Ok(ExtendedExchangeConfig {
        base_config,
        spot_symbols: codes, // Use codes for spot symbols
        futures_symbols: Vec::new(),
        krw_symbols: Vec::new(),
    })
}

// Helper function to load OKX config with categorized symbols
fn load_okx_config(path: &str) -> anyhow::Result<ExtendedExchangeConfig> {
    let file = File::open(path)?;
    let json: Value = serde_json::from_reader(file)?;
    
    let spot_symbols = json["subscribe_data"]["spot_symbols"]
        .as_array()
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
        .unwrap_or_default();
        
    let futures_symbols: Vec<String> = json["subscribe_data"]["futures_symbols"]
        .as_array()
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
        .unwrap_or_default();
        
    let perpetual_symbols: Vec<String> = json["subscribe_data"]["perpetual_symbols"]
        .as_array()
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
        .unwrap_or_default();
    
    // Combine futures and perpetual symbols
    let mut all_futures = futures_symbols;
    all_futures.extend(perpetual_symbols);
    
    let base_config = ExchangeConfig::new(path)?;
    
    Ok(ExtendedExchangeConfig {
        base_config,
        spot_symbols,
        futures_symbols: all_futures,
        krw_symbols: Vec::new(),
    })
}
