use tokio::signal;
use anyhow::Result;
use std::time::Duration;
use serde_json::Value;
use std::fs::File;

async fn run_simple_feeder() -> Result<()> {
    // Load simple config
    let config_file = File::open("config/simple_feeder.json")?;
    let config: Value = serde_json::from_reader(config_file)?;

    let exchanges: Vec<String> = config["exchanges"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let udp_mode = config["udp_mode"].as_str().unwrap_or("direct");

    if exchanges.is_empty() {
        println!("❌ No exchanges configured");
        return Err(anyhow::anyhow!("No exchanges configured"));
    }

    println!("🚀 Simple Feeder Starting");
    println!("📋 Exchanges: {:?}", exchanges);
    println!("📡 UDP Mode: {}", udp_mode);
    println!("🎯 Ready to connect to {} exchanges", exchanges.len());

    // Simple loop - replace with actual feeder logic later
    let mut counter = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                counter += 1;
                println!("⏱️  Running with {} exchanges: {:?}", exchanges.len(), exchanges);

                if counter >= 5 {
                    println!("✅ Simple feeder test completed!");
                    break;
                }
            }
            _ = signal::ctrl_c() => {
                println!("🛑 Shutting down");
                break;
            }
        }
    }

    println!("🏁 Simple feeder stopped");
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = run_simple_feeder().await {
        eprintln!("Simple feeder failed: {}", e);
    }
}