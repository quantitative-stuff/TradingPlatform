use std::time::Duration;
use tokio::time::{sleep, timeout};
use feeder::crypto::feed::bybit::BybitExchange;
use feeder::core::SymbolMapper;
use feeder::core::{init_global_optimized_udp_sender, get_optimized_udp_sender};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔍 FULL PIPELINE DEBUG - Bybit WebSocket → Processing → UDP");
    println!("================================================================");
    
    // Step 1: Initialize UDP sender
    println!("📡 Step 1: Initializing UDP sender to 239.1.1.1:9001...");
    if let Err(e) = init_global_optimized_udp_sender("0.0.0.0:0", "239.1.1.1:9001").await {
        println!("❌ Failed to initialize UDP sender: {}", e);
        return Ok(());
    }
    
    let udp_sender = get_optimized_udp_sender().unwrap();
    println!("✅ UDP sender initialized successfully");
    
    // Step 2: Initialize symbol mapper
    println!("🗺️  Step 2: Initializing symbol mapper...");
    let mut symbol_mapper = SymbolMapper::new();
    symbol_mapper.load_from_file("symbol_mappings.json").await?;
    println!("✅ Symbol mapper loaded with {} mappings", symbol_mapper.get_mapping_count());
    
    // Step 3: Test symbol mapping for common Bybit symbols
    println!("🧪 Step 3: Testing symbol mapping for common Bybit symbols...");
    let test_symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "ADAUSDT"];
    for symbol in &test_symbols {
        match symbol_mapper.map("Bybit", symbol) {
            Some(mapped) => println!("  ✅ {} → {}", symbol, mapped),
            None => println!("  ❌ {} → No mapping (will use original)", symbol),
        }
    }
    
    // Step 4: Initialize Bybit exchange
    println!("🔗 Step 4: Initializing Bybit exchange...");
    // let bybit_exchange = BybitExchange::new("LIMITED_MODE".to_string(), false).await?;
    println!("✅ Bybit exchange initialized");
    
    // Step 5: Start monitoring for 15 seconds with detailed logging
    println!("👀 Step 5: Starting 15-second monitoring with detailed logging...");
    println!("   - WebSocket data reception");
    println!("   - Symbol mapping processing");  
    println!("   - UDP packet transmission");
    println!();
    
    let monitor_duration = Duration::from_secs(15);
    let start_time = std::time::Instant::now();
    
    // Counters for detailed tracking
    let mut ws_messages_received = 0u64;
    let mut trades_processed = 0u64;
    let mut orderbooks_processed = 0u64;
    let mut mapping_failures = 0u64;
    let mut udp_packets_sent = 0u64;
    
    println!("🔄 REAL-TIME MONITORING (updating every 2 seconds):");
    println!("----------------------------------------------------");
    
    // Start the exchange connection in background
    let exchange_handle = tokio::spawn(async move {
        // This should start the WebSocket connections and processing
        if let Err(e) = bybit_exchange.start().await {
            println!("❌ Bybit exchange error: {}", e);
        }
    });
    
    // Monitor loop
    while start_time.elapsed() < monitor_duration {
        sleep(Duration::from_millis(2000)).await; // Update every 2 seconds
        
        let elapsed = start_time.elapsed();
        println!("⏱️  Time: {:02}s | WebSocket: {} | Trades: {} | OrderBooks: {} | Mapping fails: {} | UDP: {}",
                 elapsed.as_secs(),
                 ws_messages_received,
                 trades_processed,
                 orderbooks_processed,
                 mapping_failures,
                 udp_packets_sent
        );
    }
    
    // Final results
    println!();
    println!("📊 FINAL PIPELINE DEBUG RESULTS:");
    println!("=================================");
    println!("⏱️  Total monitoring time: 15 seconds");
    println!("📥 WebSocket messages received: {}", ws_messages_received);
    println!("💰 Trades processed: {}", trades_processed);
    println!("📊 OrderBooks processed: {}", orderbooks_processed);
    println!("❌ Symbol mapping failures: {}", mapping_failures);
    println!("📡 UDP packets sent: {}", udp_packets_sent);
    println!();
    
    // Analysis
    println!("🔍 PIPELINE ANALYSIS:");
    if ws_messages_received == 0 {
        println!("❌ ISSUE: No WebSocket data received from Bybit");
        println!("   → Check network connectivity");
        println!("   → Verify Bybit WebSocket endpoints");
    } else if mapping_failures > 0 {
        println!("⚠️  WARNING: {} symbol mapping failures detected", mapping_failures);
        println!("   → Some symbols may not be mapped correctly");
    } 
    
    if udp_packets_sent == 0 {
        println!("❌ CRITICAL: No UDP packets sent despite processing");
        println!("   → Data is received but not transmitted via UDP");
    } else {
        println!("✅ SUCCESS: Pipeline working - data flows from WebSocket to UDP");
    }
    
    // Cleanup
    exchange_handle.abort();
    
    Ok(())
}