use tokio::signal;
use anyhow::Result;
use std::path::PathBuf;
use std::env;
use std::fs::File;
use std::sync::Arc;
use serde_json::Value;

use feeder::core::{
    init_global_optimized_udp_sender, get_optimized_udp_sender, trigger_shutdown,
    CONNECTION_STATS, FileLogger, TerminalMonitor, SymbolMapper
};
use feeder::stock::feed::{LSExchange, FeederStock};
use feeder::load_config_ls::LSConfig;
use std::time::Duration;
use tracing::{info, warn, error};

// Stock feeder specific monitor endpoint - different from crypto (9001)
const STOCK_MONITOR_ENDPOINT: &str = "239.255.42.100:9002";

#[derive(Clone)]
struct StockExchangeConfig {
    pub base_config: LSConfig,
    pub asset_type: Vec<String>,
    pub spot_symbols: Vec<String>,
    pub future_symbols: Vec<String>,
    pub options_symbols: Vec<String>,
    pub etf_symbols: Vec<String>,
    pub elw_symbols: Vec<String>,
}

impl StockExchangeConfig {
    fn load_full_config(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let json: Value = serde_json::from_reader(file)?;

        // Load LS Securities credentials
        let app_key = json["credentials"]["app_key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing app_key"))?
            .to_string();
        let secret_key = json["credentials"]["secret_key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing secret_key"))?
            .to_string();

        let base_config = LSConfig::new(app_key, secret_key)?;

        // Extract asset types (like crypto feeder does)
        let asset_type = json["feed_config"]["asset_type"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_else(|| vec!["spot".to_string(), "future".to_string()]);

        // Extract symbols for each asset type
        let spot_symbols = json["subscribe_data"]["spot_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let future_symbols = json["subscribe_data"]["future_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let options_symbols = json["subscribe_data"]["options_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let etf_symbols = json["subscribe_data"]["etf_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let elw_symbols = json["subscribe_data"]["elw_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        Ok(Self {
            base_config,
            asset_type,
            spot_symbols,
            future_symbols,
            options_symbols,
            etf_symbols,
            elw_symbols,
        })
    }
    
    fn get_batched_symbols(&self, batch_size: usize) -> Vec<Vec<String>> {
        // Collect symbols based on asset_type (like crypto feeder does)
        let mut all_symbols = Vec::new();

        for asset_type in &self.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(self.spot_symbols.clone()),
                "future" => all_symbols.extend(self.future_symbols.clone()),
                "options" => all_symbols.extend(self.options_symbols.clone()),
                "ETF" => all_symbols.extend(self.etf_symbols.clone()),
                "ELW" => all_symbols.extend(self.elw_symbols.clone()),
                _ => warn!("LS: Unknown asset type: {}", asset_type),
            }
        }

        let mut batches = Vec::new();
        for chunk in all_symbols.chunks(batch_size) {
            batches.push(chunk.to_vec());
        }

        batches
    }
}

async fn run_ls_exchange(config: StockExchangeConfig, symbols: Vec<String>) -> Result<()> {
    let mut exchange = LSExchange::new(config.base_config.clone(), symbols.clone());
    
    info!("Starting LS Securities exchange with {} symbols", symbols.len());
    
    // Connect with retry logic
    let mut retry_count = 0;
    while retry_count < 3 {
        match exchange.connect().await {
            Ok(_) => {
                info!("Connected to LS Securities");
                break;
            }
            Err(e) => {
                error!("Failed to connect to LS Securities: {}", e);
                retry_count += 1;
                if retry_count < 3 {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                } else {
                    return Err(anyhow::anyhow!("Failed to connect after 3 attempts"));
                }
            }
        }
    }
    
    // Subscribe to symbols
    match exchange.subscribe().await {
        Ok(_) => info!("Subscribed to {} symbols", symbols.len()),
        Err(e) => {
            error!("Failed to subscribe: {}", e);
            return Err(anyhow::anyhow!("Subscription failed"));
        }
    }
    
    // Start receiving data
    if let Err(e) = exchange.start().await {
        error!("Exchange error: {}", e);
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize file logger (this also sets up tracing)
    let logger = FileLogger::new();
    logger.init().unwrap_or_else(|e| {
        eprintln!("Failed to init logger: {}", e);
        // If file logger fails, set up console logging as fallback
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    });
    
    info!("Starting Stock Feeder...");
    
    // Get config path from args or use default
    let args: Vec<String> = env::args().collect();
    let config_path = if args.len() > 1 {
        &args[1]
    } else {
        "config/stock/ls_config.json"
    };
    
    // Load configuration
    let config = StockExchangeConfig::load_full_config(config_path)?;
    info!("Loaded LS Securities configuration from {}", config_path);
    info!("Asset types: {:?}", config.asset_type);
    info!("Spot symbols: {}", config.spot_symbols.len());
    info!("Future symbols: {}", config.future_symbols.len());
    info!("Options symbols: {}", config.options_symbols.len());
    info!("ETF symbols: {}", config.etf_symbols.len());
    info!("ELW symbols: {}", config.elw_symbols.len());
    
    // Initialize UDP sender with different endpoint for stock data
    init_global_optimized_udp_sender("0.0.0.0:0", STOCK_MONITOR_ENDPOINT).await?;
    info!("Initialized UDP sender for stock data at {}", STOCK_MONITOR_ENDPOINT);
    
    // Setup shutdown handler
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let shutdown_clone = shutdown.clone();
    
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal");
                trigger_shutdown();
                shutdown_clone.notify_one();
            }
            Err(err) => error!("Unable to listen for shutdown signal: {}", err),
        }
    });
    
    // Start terminal monitor for stock feeder
    let monitor_handle = tokio::spawn(async move {
        let mut monitor = TerminalMonitor::new(true);
        let _ = monitor.start();
    });
    
    // Determine batch size for LS Securities (typically handle all in one connection)
    let batch_size = 100; // Can handle more symbols per connection than crypto
    let batches = config.get_batched_symbols(batch_size);
    
    info!("Creating {} LS Securities connections", batches.len());
    
    // Create tasks for each batch
    let mut handles = vec![];
    for (i, batch) in batches.into_iter().enumerate() {
        let config_clone = config.clone();
        let handle = tokio::spawn(async move {
            info!("Starting LS connection {} with {} symbols", i, batch.len());
            if let Err(e) = run_ls_exchange(config_clone, batch).await {
                error!("LS connection {} failed: {}", i, e);
            }
        });
        handles.push(handle);
    }
    
    // Wait for shutdown or task completion
    tokio::select! {
        _ = shutdown.notified() => {
            info!("Shutting down stock feeder...");
        }
        _ = async {
            for handle in handles {
                let _ = handle.await;
            }
        } => {
            warn!("All stock connections terminated");
        }
    }
    
    // Log shutdown
    info!("Stock Feeder shutting down");
    
    // Wait for monitor to finish
    let _ = monitor_handle.await;
    
    // Print final stats
    {
        let stats = CONNECTION_STATS.read();
        info!("Final connection statistics:");
        for (exchange, stat) in stats.iter() {
            info!("{}: connected={}, disconnected={}, reconnects={}, total_connections={}", 
                  exchange, stat.connected, stat.disconnected, stat.reconnect_count, stat.total_connections);
        }
    }
    
    Ok(())
}