use anyhow::Result;
use feeder::core::init_global_binary_udp_sender;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Testing feeder startup components...");

    // Test 1: Basic UDP sender initialization
    println!("1. Testing binary UDP sender initialization...");

    match init_global_binary_udp_sender("0.0.0.0:0", "239.255.42.99:9001").await {
        Ok(_) => println!("✅ Binary UDP sender initialized"),
        Err(e) => {
            println!("❌ Binary UDP sender failed: {}", e);
            return Err(anyhow::anyhow!("Binary UDP sender failed: {}", e));
        }
    }

    println!("2. Testing config file loading...");

    match std::fs::File::open("config/crypto/crypto_exchanges.json") {
        Ok(_) => println!("✅ Exchanges config found"),
        Err(e) => {
            println!("❌ Exchanges config failed: {}", e);
            return Err(e.into());
        }
    }

    match std::fs::File::open("config/crypto/symbol_mapping.json") {
        Ok(_) => println!("✅ Symbol mapping found"),
        Err(e) => {
            println!("❌ Symbol mapping failed: {}", e);
            return Err(e.into());
        }
    }

    println!("✅ All startup components working!");

    Ok(())
}