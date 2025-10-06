use std::path::PathBuf;
use std::env;
use std::fs::File;
use serde_json::Value;
use feeder::load_config::ExchangeConfig;

#[derive(Clone)]
struct ExtendedExchangeConfig {
    pub base_config: ExchangeConfig,
    pub spot_symbols: Vec<String>,
    pub futures_symbols: Vec<String>,
    pub krw_symbols: Vec<String>,
}

impl ExtendedExchangeConfig {
    fn load_full_config(path: &str) -> anyhow::Result<Self> {
        let file = File::open(path)?;
        let json: Value = serde_json::from_reader(file)?;

        let spot_symbols = json["subscribe_data"]["spot_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let futures_symbols = json["subscribe_data"]["futures_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let krw_symbols = json["subscribe_data"]["krw_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        // Use the same full config file for base configuration
        let base_config = ExchangeConfig::new(path)?;

        Ok(Self {
            base_config,
            spot_symbols,
            futures_symbols,
            krw_symbols,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 Debug Feeder Direct - Understanding symbol loading");

    // Read exchanges from config file
    let config_file = std::fs::File::open("config/crypto/crypto_exchanges.json")?;
    let config: serde_json::Value = serde_json::from_reader(config_file)?;
    let selected_exchanges: Vec<String> = config["exchanges"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    println!("📋 Selected exchanges: {:?}", selected_exchanges);

    // Set up config directory for exchange configs
    let config_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
        .join("config")
        .join("crypto");

    for exchange_name in &selected_exchanges {
        match exchange_name.as_str() {
            "okx" => {
                println!("\n🔍 Loading OKX config...");
                let config_path = config_dir.join("okx_config_full.json");
                println!("   Config path: {}", config_path.display());

                if let Ok(config) = ExtendedExchangeConfig::load_full_config(
                    config_path.to_str().unwrap()
                ) {
                    println!("✅ OKX config loaded successfully!");
                    println!("   Exchange: {}", config.base_config.feed_config.exchange);
                    println!("   Asset types: {:?}", config.base_config.feed_config.asset_type);

                    // Show what symbols from base_config.codes
                    println!("\n📄 Base config 'codes' array ({} symbols):", config.base_config.subscribe_data.codes.len());
                    for (i, symbol) in config.base_config.subscribe_data.codes.iter().take(5).enumerate() {
                        println!("   [{}] {}", i, symbol);
                    }
                    if config.base_config.subscribe_data.codes.len() > 5 {
                        println!("   ... and {} more", config.base_config.subscribe_data.codes.len() - 5);
                    }

                    // Show what symbols are in spot_symbols array
                    println!("\n💰 Extended spot_symbols array ({} symbols):", config.spot_symbols.len());
                    for (i, symbol) in config.spot_symbols.iter().take(5).enumerate() {
                        println!("   [{}] {}", i, symbol);
                    }
                    if config.spot_symbols.len() > 5 {
                        println!("   ... and {} more", config.spot_symbols.len() - 5);
                    }

                    // Show what symbols are in futures_symbols array
                    println!("\n🚀 Extended futures_symbols array ({} symbols):", config.futures_symbols.len());
                    for (i, symbol) in config.futures_symbols.iter().take(5).enumerate() {
                        println!("   [{}] {}", i, symbol);
                    }
                    if config.futures_symbols.len() > 5 {
                        println!("   ... and {} more", config.futures_symbols.len() - 5);
                    }

                    // Show how feeder_direct would combine them
                    let mut all_symbols = Vec::new();
                    for asset_type in &config.base_config.feed_config.asset_type {
                        match asset_type.as_str() {
                            "spot" => {
                                println!("\n✅ Asset type 'spot' -> adding {} symbols from spot_symbols", config.spot_symbols.len());
                                all_symbols.extend(config.spot_symbols.clone());
                            },
                            "swap" | "futures" => {
                                println!("✅ Asset type '{}' -> adding {} symbols from futures_symbols", asset_type, config.futures_symbols.len());
                                all_symbols.extend(config.futures_symbols.clone());
                            },
                            _ => {
                                println!("⚠️  Unknown asset type: {}", asset_type);
                            }
                        }
                    }

                    println!("\n🎯 Final symbols for feeder_direct ({} total):", all_symbols.len());
                    for (i, symbol) in all_symbols.iter().take(10).enumerate() {
                        println!("   [{}] {}", i, symbol);
                    }
                    if all_symbols.len() > 10 {
                        println!("   ... and {} more", all_symbols.len() - 10);
                    }

                    println!("\n🔍 DIAGNOSIS:");
                    if all_symbols.is_empty() {
                        println!("❌ No symbols would be loaded! Check asset_type configuration.");
                    } else {
                        // Check if symbols are USD vs USDT format
                        let has_usd = all_symbols.iter().any(|s| s.contains("-USD") && !s.contains("-USDT"));
                        let has_usdt = all_symbols.iter().any(|s| s.contains("-USDT"));

                        if has_usd && !has_usdt {
                            println!("❌ All symbols are USD format (e.g., BTC-USD) but OKX doesn't support USD pairs!");
                            println!("   OKX requires USDT format (e.g., BTC-USDT)");
                        } else if has_usdt {
                            println!("✅ Symbols are in USDT format - should work with OKX");
                        } else {
                            println!("⚠️  Symbols don't match expected USD/USDT pattern");
                        }
                    }

                } else {
                    println!("❌ Failed to load OKX config");
                }
            },
            _ => {
                println!("⚠️  Skipping exchange: {}", exchange_name);
            }
        }
    }

    Ok(())
}