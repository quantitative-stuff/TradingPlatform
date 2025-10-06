use tokio::signal;
use anyhow::Result;
use std::env;

#[tokio::main]
async fn main() {
    // Basic debug feeder to identify where the hang occurs

    println!("🚀 Debug feeder starting...");

    // Get exchanges from environment variable
    let exchanges_env = env::var("EXCHANGES")
        .unwrap_or_else(|_| "binance".to_string());

    println!("📋 EXCHANGES environment variable: {}", exchanges_env);

    let selected_exchanges: Vec<&str> = exchanges_env
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if selected_exchanges.is_empty() {
        println!("❌ No exchanges specified");
        return;
    }

    println!("✅ Selected exchanges: {:?}", selected_exchanges);

    // Check config files
    println!("📂 Checking config files...");

    let config_dir = std::path::PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
        .join("config");

    println!("📁 Config directory: {}", config_dir.display());

    let symbol_mapper_path = config_dir.join("crypto/symbol_mapping.json");
    if !symbol_mapper_path.exists() {
        println!("❌ symbol_mapping.json not found at: {}", symbol_mapper_path.display());
        return;
    }

    println!("✅ symbol_mapping.json found");

    // Check exchange configs
    for exchange in &selected_exchanges {
        let config_path = config_dir.join(format!("{}_config_full.json", exchange));
        if config_path.exists() {
            println!("✅ {}_config_full.json found", exchange);
        } else {
            println!("❌ {}_config_full.json NOT found", exchange);
        }
    }

    println!("🔧 Testing file operations...");

    // Test reading symbol mapper
    match std::fs::File::open(&symbol_mapper_path) {
        Ok(_) => println!("✅ Can open symbol_mapping.json"),
        Err(e) => {
            println!("❌ Cannot open symbol_mapping.json: {}", e);
            return;
        }
    }

    println!("🌐 Testing network operations...");

    // Test basic network connectivity
    match tokio::time::timeout(std::time::Duration::from_secs(5), test_network()).await {
        Ok(Ok(())) => println!("✅ Network test passed"),
        Ok(Err(e)) => println!("❌ Network test failed: {}", e),
        Err(_) => println!("⏰ Network test timed out"),
    }

    println!("💾 Testing file logging...");

    // Test file logging
    match test_logging().await {
        Ok(()) => println!("✅ File logging test passed"),
        Err(e) => println!("❌ File logging test failed: {}", e),
    }

    println!("🗄️ Testing database connections...");

    // Test database connections
    match tokio::time::timeout(std::time::Duration::from_secs(3), test_database()).await {
        Ok(Ok(())) => println!("✅ Database test passed"),
        Ok(Err(e)) => println!("⚠️ Database test failed (might be expected): {}", e),
        Err(_) => println!("⏰ Database test timed out"),
    }

    println!("🔄 Starting main feeder loop...");

    // Simple async loop to test if basic tokio works
    let mut counter = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                counter += 1;
                println!("⏱️ Heartbeat {}", counter);

                if counter >= 10 {
                    println!("✅ Main loop test completed successfully");
                    break;
                }
            }
            _ = signal::ctrl_c() => {
                println!("🛑 Ctrl+C received, shutting down");
                break;
            }
        }
    }

    println!("🏁 Debug feeder completed");
}

async fn test_network() -> Result<()> {
    // Test basic DNS resolution
    let _addr = tokio::net::lookup_host("google.com:80").await?;
    Ok(())
}

async fn test_logging() -> Result<()> {
    use std::fs;

    // Create logs directory
    fs::create_dir_all("logs")?;

    // Test writing to a log file
    let log_file = "logs/debug_test.log";
    std::fs::write(log_file, "Debug test log entry\n")?;

    Ok(())
}

async fn test_database() -> Result<()> {
    // Try to create a basic QuestDB config
    let questdb_config = market_feeder::connect_to_databse::QuestDBConfig::default();

    // Try to connect (this might fail, which is okay for testing)
    let _client = market_feeder::connect_to_databse::QuestDBClient::new(questdb_config).await?;

    Ok(())
}