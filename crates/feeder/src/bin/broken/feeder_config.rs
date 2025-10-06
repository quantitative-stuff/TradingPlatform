use tokio::signal;
use anyhow::Result;
use std::env;
use std::time::Duration;

use feeder::feeder_config::FeederConfig;

async fn run_feeder_with_config() -> Result<()> {
    // Load feeder configuration
    let config_path = env::args().nth(1).unwrap_or_else(|| "config/shared/feeder_config.json".to_string());

    let mut feeder_config = match FeederConfig::load(&config_path) {
        Ok(config) => config,
        Err(e) => {
            println!("❌ Failed to load feeder config from {}: {}", config_path, e);
            return Err(anyhow::anyhow!("Failed to load feeder config"));
        }
    };

    // Check for profile override from command line
    if let Some(profile) = env::args().nth(2) {
        if profile.starts_with("--profile=") {
            let profile_name = profile.strip_prefix("--profile=").unwrap();
            feeder_config.set_exchange_profile(profile_name)?;
            println!("🔄 Using exchange profile: {}", profile_name);
        }
    }

    // Validate configuration
    feeder_config.validate()?;

    // Print configuration summary
    market_feeder::config::feeder_config::print_config_info(&feeder_config);

    let selected_exchanges = feeder_config.get_enabled_exchanges();
    if selected_exchanges.is_empty() {
        println!("❌ No exchanges enabled in configuration");
        return Err(anyhow::anyhow!("No exchanges enabled"));
    }

    println!("🚀 Starting config-based feeder with {} exchanges", selected_exchanges.len());
    println!("📡 UDP Mode: {} -> {}",
             feeder_config.feeder.udp_mode,
             feeder_config.feeder.udp_settings.multicast_address);

    // Simple demonstration loop
    let mut counter = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                counter += 1;
                println!("⏱️ Config feeder heartbeat {} (exchanges: {:?})", counter, selected_exchanges);

                if counter >= 5 {
                    println!("✅ Config feeder test completed successfully!");
                    break;
                }
            }
            _ = signal::ctrl_c() => {
                println!("🛑 Ctrl+C received, shutting down");
                break;
            }
        }
    }

    println!("🏁 Config-based feeder stopped cleanly");
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = run_feeder_with_config().await {
        eprintln!("Config-based feeder failed: {}", e);
    }
}