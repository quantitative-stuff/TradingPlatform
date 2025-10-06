use feeder::crypto::feed::okx::OKXExchange;
use feeder::core::{Feeder, init_global_binary_udp_sender};
use feeder::load_config::load_exchange_config;
use tokio;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("market_feeder=debug,test_okx_direct=info")
        .init();

    println!("Starting OKX Direct Test...");

    // Initialize UDP sender
    init_global_binary_udp_sender("239.255.42.99:9001".to_string()).await?;
    println!("✅ UDP sender initialized");

    // Load OKX config
    let config_path = "config/crypto/okx_config.json";
    let config = load_exchange_config(config_path)?;
    println!("✅ Config loaded: {} symbols", config.symbols.len());

    // Create OKX exchange
    let okx = OKXExchange::new(config.clone());
    println!("✅ OKX exchange created");

    // Start the exchange feeder
    println!("Starting OKX feeder...");
    okx.start("spot").await;

    println!("OKX feeder started. Check UDP monitor for data flow.");

    // Keep running
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

    Ok(())
}