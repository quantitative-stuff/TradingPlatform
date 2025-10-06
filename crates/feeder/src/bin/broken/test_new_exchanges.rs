use tokio::signal;
use anyhow::Result;
use std::path::PathBuf;
use std::env;
use std::sync::Arc;

use feeder::core::{Feeder, SymbolMapper, CONNECTION_STATS, FileLogger, init_global_optimized_udp_sender};
use feeder::crypto::feed::{
    CoinbaseExchange, DeribitExchange, OkxExchange, BithumbExchange
};
use feeder::load_config::ExchangeConfig;
use std::time::Duration;
use tracing::{info, warn, error};
use colored::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .with_target(false)
        .init();

    println!("{}", "========================================".bright_cyan());
    println!("{}", "  NEW EXCHANGES CONNECTION TEST".bright_white().bold());
    println!("{}", "========================================".bright_cyan());
    println!();

    // Initialize UDP sender for monitoring
    init_global_optimized_udp_sender("0.0.0.0:0", "127.0.0.1:9001").await?;
    
    // Get config directory
    let config_dir = PathBuf::from("src/config");
    
    // Load symbol mapper
    let symbol_mapper_path = config_dir.join("crypto/symbol_mapping.json");
    if !symbol_mapper_path.exists() {
        return Err(anyhow::anyhow!("symbol_mapping.json not found"));
    }
    let symbol_mapper: SymbolMapper = serde_json::from_reader(File::open(symbol_mapper_path.to_str().unwrap())?)?;
    let symbol_mapper = Arc::new(symbol_mapper);
    
    // Test configuration - limit symbols for testing
    let test_mode = env::var("TEST_MODE").unwrap_or("true".to_string()) == "true";
    
    println!("{}", "Select exchange to test:".bright_yellow());
    println!("1. Coinbase (USD pairs)");
    println!("2. Deribit (Derivatives)");
    println!("3. OKX (Spot & Perpetuals)");
    println!("4. Bithumb (KRW pairs)");
    println!("5. All exchanges");
    println!();
    print!("Enter choice (1-5): ");
    
    use std::io::{self, Write};
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice = input.trim().parse::<u32>().unwrap_or(5);
    
    println!();
    
    // Initialize file logger
    let logger = FileLogger::new();
    if let Err(e) = logger.init() {
        eprintln!("Failed to initialize file logger: {}", e);
        return Err(anyhow::anyhow!("Logging init failed"));
    }
    
    let mut handles = vec![];
    
    // Test Coinbase
    if choice == 1 || choice == 5 {
        println!("{}", "Testing COINBASE...".bright_green().bold());
        
        // Create limited config for testing
        let config_path = if test_mode {
            // Create test config with only 2 symbols
            create_test_config("coinbase", vec!["BTC-USD", "ETH-USD"])?
        } else {
            config_dir.join("coinbase_config.json")
        };
        
        let config = ExchangeConfig::new(config_path.to_str().unwrap())?;
        let mut exchange = CoinbaseExchange::new(config, symbol_mapper.clone());
        
        let handle = tokio::spawn(async move {
            info!("Starting Coinbase feeder...");
            
            if let Err(e) = exchange.connect().await {
                error!("Coinbase connect error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.subscribe().await {
                error!("Coinbase subscribe error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.start().await {
                error!("Coinbase start error: {}", e);
            }
        });
        handles.push(handle);
        
        // Give it time to connect
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    
    // Test Deribit
    if choice == 2 || choice == 5 {
        println!("{}", "Testing DERIBIT...".bright_green().bold());
        
        let config_path = if test_mode {
            create_test_config("deribit", vec!["BTC-PERPETUAL", "ETH-PERPETUAL"])?
        } else {
            config_dir.join("deribit_config.json")
        };
        
        let config = ExchangeConfig::new(config_path.to_str().unwrap())?;
        let mut exchange = DeribitExchange::new(config, symbol_mapper.clone());
        
        let handle = tokio::spawn(async move {
            info!("Starting Deribit feeder...");
            
            if let Err(e) = exchange.connect().await {
                error!("Deribit connect error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.subscribe().await {
                error!("Deribit subscribe error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.start().await {
                error!("Deribit start error: {}", e);
            }
        });
        handles.push(handle);
        
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    
    // Test OKX
    if choice == 3 || choice == 5 {
        println!("{}", "Testing OKX...".bright_green().bold());
        
        let config_path = if test_mode {
            create_test_config("okx", vec!["BTC-USDT", "ETH-USDT"])?
        } else {
            config_dir.join("okx_config.json")
        };
        
        let config = ExchangeConfig::new(config_path.to_str().unwrap())?;
        let mut exchange = OkxExchange::new(config, symbol_mapper.clone());
        
        let handle = tokio::spawn(async move {
            info!("Starting OKX feeder...");
            
            if let Err(e) = exchange.connect().await {
                error!("OKX connect error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.subscribe().await {
                error!("OKX subscribe error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.start().await {
                error!("OKX start error: {}", e);
            }
        });
        handles.push(handle);
        
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    
    // Test Bithumb
    if choice == 4 || choice == 5 {
        println!("{}", "Testing BITHUMB...".bright_green().bold());
        
        let config_path = if test_mode {
            create_test_config("bithumb", vec!["BTC_KRW", "ETH_KRW"])?
        } else {
            config_dir.join("bithumb_config.json")
        };
        
        let config = ExchangeConfig::new(config_path.to_str().unwrap())?;
        let mut exchange = BithumbExchange::new(config, symbol_mapper.clone());
        
        let handle = tokio::spawn(async move {
            info!("Starting Bithumb feeder...");
            
            if let Err(e) = exchange.connect().await {
                error!("Bithumb connect error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.subscribe().await {
                error!("Bithumb subscribe error: {}", e);
                return;
            }
            
            if let Err(e) = exchange.start().await {
                error!("Bithumb start error: {}", e);
            }
        });
        handles.push(handle);
        
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    
    println!();
    println!("{}", "========================================".bright_cyan());
    println!("{}", "  MONITORING CONNECTION STATUS".bright_white().bold());
    println!("{}", "========================================".bright_cyan());
    println!();
    
    // Monitor connection status
    let monitor_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            
            // Clear screen for update
            print!("\x1B[2J\x1B[1;1H");
            
            println!("{}", "CONNECTION STATUS:".bright_yellow().bold());
            println!("{}", "─────────────────".bright_white());
            
            let stats = CONNECTION_STATS.read();
            
            if stats.is_empty() {
                println!("{}", "No connections yet...".dim());
            } else {
                for (exchange, stat) in stats.iter() {
                    let status = if stat.connected > 0 {
                        format!("● CONNECTED ({})", stat.connected).green()
                    } else {
                        "○ DISCONNECTED".red()
                    };
                    
                    println!("{:<20} {}", 
                        exchange.bright_white(), 
                        status
                    );
                    
                    if stat.total_connections > 0 {
                        println!("  Total: {} | Disconnected: {} | Reconnects: {}", 
                            stat.total_connections,
                            stat.disconnected,
                            stat.reconnect_count
                        );
                    }
                }
            }
            
            println!();
            println!("{}", "Press Ctrl+C to stop...".dim());
        }
    });
    
    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            println!("\n{}", "Shutting down test...".bright_red());
        }
        _ = monitor_handle => {}
    }
    
    // Wait for tasks to complete
    for handle in handles {
        handle.abort();
    }
    
    println!("{}", "Test completed!".bright_green().bold());
    Ok(())
}

fn create_test_config(exchange: &str, symbols: Vec<&str>) -> Result<PathBuf> {
    use std::fs;
    use serde_json::json;
    
    let config = match exchange {
        "coinbase" => json!({
            "connect_config": {
                "ws_url": "wss://advanced-trade-ws.coinbase.com",
                "requires_auth": false,
                "timeout_secs": 10,
                "initial_retry_delay_secs": 5,
                "max_retry_delay_secs": 300,
                "connection_delay_ms": 100
            },
            "subscribe_data": {
                "codes": symbols,
                "stream_type": ["market_trades", "level2", "ticker"]
            },
            "feed_config": {
                "asset_type": ["spot"],
                "feed_data_type": ["trade", "orderbook"]
            }
        }),
        "deribit" => json!({
            "connect_config": {
                "ws_url": "wss://www.deribit.com/ws/api/v2",
                "timeout_secs": 10,
                "initial_retry_delay_secs": 5,
                "max_retry_delay_secs": 300,
                "connection_delay_ms": 200
            },
            "subscribe_data": {
                "codes": symbols,
                "stream_type": ["trades", "book", "ticker"]
            },
            "feed_config": {
                "asset_type": ["perpetual"],
                "feed_data_type": ["trade", "orderbook"]
            }
        }),
        "okx" => json!({
            "connect_config": {
                "ws_url": "wss://ws.okx.com:8443/ws/v5/public",
                "timeout_secs": 10,
                "initial_retry_delay_secs": 5,
                "max_retry_delay_secs": 300,
                "connection_delay_ms": 100
            },
            "subscribe_data": {
                "codes": symbols,
                "stream_type": ["trades", "books", "tickers"]
            },
            "feed_config": {
                "asset_type": ["spot"],
                "feed_data_type": ["trade", "orderbook"]
            }
        }),
        "bithumb" => json!({
            "connect_config": {
                "ws_url": "wss://pubwss.bithumb.com/pub/ws",
                "timeout_secs": 10,
                "initial_retry_delay_secs": 10,
                "max_retry_delay_secs": 600,
                "connection_delay_ms": 500
            },
            "subscribe_data": {
                "codes": symbols,
                "stream_type": ["transaction"]
            },
            "feed_config": {
                "asset_type": ["spot"],
                "feed_data_type": ["trade"]
            }
        }),
        _ => return Err(anyhow::anyhow!("Unknown exchange"))
    };
    
    let path = PathBuf::from(format!("test_{}_config.json", exchange));
    fs::write(&path, config.to_string())?;
    Ok(path)
}