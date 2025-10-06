use anyhow::Result;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{Write, Read};

/// Standalone binary to organize stock market tickers for LS Securities
///
/// Since LS Securities REST API requires proprietary 'ebest' Python library,
/// this process reads from existing k100_stock_full.json and reorganizes
/// tickers by asset type: spot (stocks), futures, ETF, ELW, options

const INPUT_FILE: &str = "config/stock/k100_stock_full.json";
const OUTPUT_DIR: &str = "config/stock/ls_sec";

fn save_to_file(filename: &str, symbols: &[String]) -> Result<()> {
    fs::create_dir_all(OUTPUT_DIR)?;

    let filepath = format!("{}/{}", OUTPUT_DIR, filename);
    let mut file = File::create(&filepath)?;

    let json = serde_json::to_string_pretty(&symbols)?;
    file.write_all(json.as_bytes())?;

    println!("✓ Saved {} symbols to {}", symbols.len(), filepath);
    Ok(())
}

fn main() -> Result<()> {
    println!("========================================");
    println!("   STOCK TICKER ORGANIZER (LS Securities)");
    println!("========================================\n");

    // Read the k100_stock_full.json file
    println!("📖 Reading from {}...", INPUT_FILE);
    let mut file = File::open(INPUT_FILE)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let securities: Vec<Value> = serde_json::from_str(&contents)?;
    println!("✓ Loaded {} securities\n", securities.len());

    // Extract symbols by type
    println!("🔍 Organizing by asset type...");

    let mut spot_symbols = Vec::new();
    let mut future_symbols = Vec::new();
    let mut etf_symbols = Vec::new();
    let mut elw_symbols = Vec::new();

    for security in &securities {
        // Check if this entry has monthly_future field (indicates it's a future contract)
        if security.get("monthly_future").is_some() {
            if let Some(shcode) = security["shcode"].as_str() {
                future_symbols.push(shcode.to_string());
            }
            continue;
        }

        // Otherwise, it's a spot security
        let basecode = security["basecode"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('A')
            .to_string();

        if basecode.is_empty() {
            continue;
        }

        let asset_type = security["asset_type"]
            .as_str()
            .unwrap_or("KOSPI_STOCK");

        // Categorize by asset type
        if asset_type.contains("ETF") {
            etf_symbols.push(basecode);
        } else if asset_type.contains("ELW") {
            elw_symbols.push(basecode);
        } else if asset_type.contains("STOCK") {
            spot_symbols.push(basecode);
        }
    }

    // Remove duplicates
    spot_symbols.sort();
    spot_symbols.dedup();
    future_symbols.sort();
    future_symbols.dedup();
    etf_symbols.sort();
    etf_symbols.dedup();
    elw_symbols.sort();
    elw_symbols.dedup();

    println!("  - Spot (stocks): {}", spot_symbols.len());
    println!("  - Futures: {}", future_symbols.len());
    println!("  - ETF: {}", etf_symbols.len());
    println!("  - ELW: {}", elw_symbols.len());
    println!();

    // Save to files
    println!("💾 Saving to files...");
    save_to_file("spot_symbols.json", &spot_symbols)?;
    save_to_file("future_symbols.json", &future_symbols)?;
    save_to_file("etf_symbols.json", &etf_symbols)?;
    save_to_file("elw_symbols.json", &elw_symbols)?;

    println!("\n✅ Ticker organization completed!");
    println!("Files saved to: {}/", OUTPUT_DIR);

    Ok(())
}