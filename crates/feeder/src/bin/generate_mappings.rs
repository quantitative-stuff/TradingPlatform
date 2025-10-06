use std::fs::File;
use std::io::Write;
use std::collections::{HashMap, HashSet};
use serde_json::{Value, json};
use anyhow::Result;

fn extract_symbols_from_config(path: &str) -> Result<HashSet<String>> {
    let file = File::open(path)?;
    let json: Value = serde_json::from_reader(file)?;
    let mut symbols = HashSet::new();
    
    // Extract spot symbols
    if let Some(spot) = json["subscribe_data"]["spot_symbols"].as_array() {
        for symbol in spot {
            if let Some(s) = symbol.as_str() {
                symbols.insert(s.to_string());
            }
        }
    }
    
    // Extract futures symbols
    if let Some(futures) = json["subscribe_data"]["futures_symbols"].as_array() {
        for symbol in futures {
            if let Some(s) = symbol.as_str() {
                symbols.insert(s.to_string());
            }
        }
    }
    
    // Extract KRW symbols (for Upbit)
    if let Some(krw) = json["subscribe_data"]["krw_symbols"].as_array() {
        for symbol in krw {
            if let Some(s) = symbol.as_str() {
                symbols.insert(s.to_string());
            }
        }
    }
    
    // Extract from codes (old format)
    if let Some(codes) = json["subscribe_data"]["codes"].as_array() {
        for code in codes {
            if let Some(s) = code.as_str() {
                symbols.insert(s.to_string());
            }
        }
    }
    
    Ok(symbols)
}

fn convert_to_common_format(exchange: &str, symbol: &str) -> String {
    match exchange {
        "Binance" => {
            // BTCUSDT -> BTC-USDT
            if symbol.ends_with("USDT") {
                let base = &symbol[..symbol.len() - 4];
                format!("{}-USDT", base)
            } else if symbol.ends_with("BUSD") {
                let base = &symbol[..symbol.len() - 4];
                format!("{}-BUSD", base)
            } else if symbol.ends_with("BTC") {
                let base = &symbol[..symbol.len() - 3];
                format!("{}-BTC", base)
            } else if symbol.ends_with("ETH") {
                let base = &symbol[..symbol.len() - 3];
                format!("{}-ETH", base)
            } else if symbol.ends_with("BNB") {
                let base = &symbol[..symbol.len() - 3];
                format!("{}-BNB", base)
            } else {
                symbol.to_string() // Keep as is if unknown pattern
            }
        },
        "Bybit" => {
            // BTCUSDT -> BTC-USDT (same as Binance)
            if symbol.ends_with("USDT") {
                let base = &symbol[..symbol.len() - 4];
                format!("{}-USDT", base)
            } else if symbol.ends_with("USDC") {
                let base = &symbol[..symbol.len() - 4];
                format!("{}-USDC", base)
            } else if symbol.ends_with("BTC") {
                let base = &symbol[..symbol.len() - 3];
                format!("{}-BTC", base)
            } else if symbol.ends_with("ETH") {
                let base = &symbol[..symbol.len() - 3];
                format!("{}-ETH", base)
            } else {
                symbol.to_string()
            }
        },
        "Upbit" => {
            // KRW-BTC -> BTC-KRW
            if symbol.starts_with("KRW-") {
                let base = &symbol[4..];
                format!("{}-KRW", base)
            } else if symbol.starts_with("USDT-") {
                let base = &symbol[5..];
                format!("{}-USDT", base)
            } else if symbol.starts_with("BTC-") {
                let base = &symbol[4..];
                format!("{}-BTC", base)
            } else {
                symbol.to_string()
            }
        },
        _ => symbol.to_string()
    }
}

fn main() -> Result<()> {
    println!("Generating symbol mappings...\n");
    
    let config_dir = "src/config";
    
    // Load existing mappings
    let existing_path = format!("{}/crypto/symbol_mapping.json", config_dir);
    let existing_file = File::open(&existing_path)?;
    let mut mapping: Value = serde_json::from_reader(existing_file)?;
    
    // Process each exchange
    let exchanges = vec![
        ("Binance", vec!["binance_config_full.json", "binance_config.json"]),
        ("Bybit", vec!["bybit_config_full.json", "bybit_config.json"]),
        ("Upbit", vec!["upbit_config_full.json", "upbit_config.json"]),
    ];
    
    for (exchange_name, config_files) in exchanges {
        println!("Processing {} configs...", exchange_name);
        let mut all_symbols = HashSet::new();
        
        for config_file in config_files {
            let path = format!("{}/{}", config_dir, config_file);
            match extract_symbols_from_config(&path) {
                Ok(symbols) => {
                    println!("  Found {} symbols in {}", symbols.len(), config_file);
                    all_symbols.extend(symbols);
                },
                Err(e) => {
                    println!("  Warning: Could not read {}: {}", config_file, e);
                }
            }
        }
        
        // Generate mappings
        let mut exchange_mappings = HashMap::new();
        for symbol in all_symbols {
            let common = convert_to_common_format(exchange_name, &symbol);
            exchange_mappings.insert(symbol, common);
        }
        
        // Update the mapping JSON
        if !exchange_mappings.is_empty() {
            mapping["exchanges"][exchange_name] = json!(exchange_mappings);
            println!("  Generated {} mappings for {}", exchange_mappings.len(), exchange_name);
        }
    }
    
    // Save the updated mappings
    let output_path = format!("{}/symbol_mapping_complete.json", config_dir);
    let mut output_file = File::create(&output_path)?;
    let json_string = serde_json::to_string_pretty(&mapping)?;
    output_file.write_all(json_string.as_bytes())?;
    
    println!("\n✅ Complete mappings saved to {}", output_path);
    println!("To use it, rename it to crypto/symbol_mapping.json");
    
    Ok(())
}