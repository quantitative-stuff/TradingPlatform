use feeder::load_config_ls::LSConfig;
use feeder::stock::feed::{LSExchange, FeederStock};
use tokio::time::{timeout, Duration};
use chrono::Timelike;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 LS Securities Simple Connection Test");
    println!("========================================");
    
    // Load config
    let config = LSConfig::from_secure_file("./docs/api/ls_sec_keys.txt")?;
    println!("✅ Keys loaded successfully");
    
    // Create exchange with minimal symbols for testing
    let test_symbols = vec!["005930".to_string()]; // Samsung Electronics only
    let mut exchange = LSExchange::new(config, test_symbols.clone());
    
    // Connect (authenticate + websocket)
    println!("\n1️⃣ Connecting to LS Securities...");
    exchange.connect().await?;
    println!("✅ Connected successfully");
    
    // Subscribe
    println!("\n2️⃣ Subscribing to symbols...");
    exchange.subscribe().await?;
    println!("✅ Subscribed successfully");
    
    // Start receiving data with timeout
    println!("\n3️⃣ Starting data reception (30 second test)...");
    println!("Note: Market hours are 09:00-15:30 KST (Korean Standard Time)");
    
    let now = chrono::Utc::now();
    let kst_time = now + chrono::Duration::hours(9);
    println!("Current time (KST): {}", kst_time.format("%H:%M:%S"));
    
    // Check if market is open (rough check)
    let hour = kst_time.hour();
    let minute = kst_time.minute();
    let is_market_hours = (hour == 9 && minute >= 0) || 
                          (hour > 9 && hour < 15) || 
                          (hour == 15 && minute <= 30);
    
    if !is_market_hours {
        println!("⚠️  Market appears to be closed. You may not receive real-time data.");
        println!("   Market hours: 09:00-15:30 KST (00:00-06:30 UTC)");
    } else {
        println!("✅ Market should be open - expecting real-time data");
    }
    
    // Run for 30 seconds
    match timeout(Duration::from_secs(30), exchange.start()).await {
        Ok(result) => {
            match result {
                Ok(_) => println!("\n✅ Data reception completed normally"),
                Err(e) => println!("\n❌ Error during data reception: {}", e),
            }
        }
        Err(_) => {
            println!("\n⏱️ 30-second test period completed");
        }
    }
    
    println!("\n📊 Test Summary:");
    println!("- Authentication: ✅");
    println!("- WebSocket Connection: ✅");
    println!("- Subscription: ✅");
    println!("- Data Reception: Check output above");
    
    Ok(())
}