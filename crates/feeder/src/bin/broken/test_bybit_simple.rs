use feeder::crypto::feed::BybitExchange;
use feeder::load_config::ExchangeConfig;
use feeder::core::{Feeder, SymbolMapper, FileLogger};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Setup file logger
    let logger = FileLogger::new();
    logger.init().expect("Failed to init logger");
    
    println!("Testing Bybit with minimal config...");
    
    // Use the full config
    let config = ExchangeConfig::new("config/crypto/bybit_config_full.json")
        .expect("Failed to load config");
    
    let spot_symbols = config.subscribe_data.spot_symbols.as_ref().map(|s| s.len()).unwrap_or(0);
    let futures_symbols = config.subscribe_data.futures_symbols.as_ref().map(|s| s.len()).unwrap_or(0);
    println!("Loaded config with {} spot + {} futures symbols", spot_symbols, futures_symbols);
    
    let symbol_mapper = Arc::new(SymbolMapper::new());
    let mut exchange = BybitExchange::new(config, symbol_mapper);
    
    println!("\nConnecting to Bybit...");
    match exchange.connect().await {
        Ok(_) => println!("✓ Connected successfully"),
        Err(e) => {
            println!("✗ Connection failed: {}", e);
            return;
        }
    }
    
    println!("\nSubscribing to streams...");
    match exchange.subscribe().await {
        Ok(_) => println!("✓ Subscribed successfully"),
        Err(e) => {
            println!("✗ Subscription failed: {}", e);
            return;
        }
    }
    
    println!("\nStarting feed (will run for 10 seconds)...");
    
    // Run the feed in a separate task
    let feed_handle = tokio::spawn(async move {
        if let Err(e) = exchange.start().await {
            eprintln!("Feed error: {}", e);
        }
    });
    
    // Wait for 10 seconds to see if we get data
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    
    println!("\nTest complete. Check logs/feeder_*.log for details.");
    
    // Cancel the feed task
    feed_handle.abort();
}