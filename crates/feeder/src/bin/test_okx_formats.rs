use reqwest;
use serde_json::Value;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Fetching OKX symbol formats...\n");

    // Fetch a few spot symbols
    println!("SPOT symbols (first 5):");
    let spot_url = "https://www.okx.com/api/v5/public/instruments?instType=SPOT";
    let spot_response = reqwest::get(spot_url).await?;
    let spot_data: Value = spot_response.json().await?;

    if let Some(data) = spot_data["data"].as_array() {
        for (i, item) in data.iter().take(5).enumerate() {
            if item["state"].as_str() == Some("live") {
                let inst_id = item["instId"].as_str().unwrap_or("");
                let base = item["baseCcy"].as_str().unwrap_or("");
                let quote = item["quoteCcy"].as_str().unwrap_or("");
                println!("  {}. {} (base: {}, quote: {})", i+1, inst_id, base, quote);
            }
        }
    }

    // Fetch a few swap symbols
    println!("\nSWAP symbols (perpetual futures, first 5):");
    let swap_url = "https://www.okx.com/api/v5/public/instruments?instType=SWAP";
    let swap_response = reqwest::get(swap_url).await?;
    let swap_data: Value = swap_response.json().await?;

    if let Some(data) = swap_data["data"].as_array() {
        for (i, item) in data.iter().take(5).enumerate() {
            if item["state"].as_str() == Some("live") {
                let inst_id = item["instId"].as_str().unwrap_or("");
                let underlying = item["uly"].as_str().unwrap_or("");
                let settle = item["settleCcy"].as_str().unwrap_or("");
                let ct_type = item["ctType"].as_str().unwrap_or("");
                println!("  {}. {} (underlying: {}, settle: {}, type: {})",
                    i+1, inst_id, underlying, settle, ct_type);
            }
        }
    }

    // Check if BTC-USDT exists in both
    println!("\nChecking for BTC-USDT in both markets:");

    // Check spot
    if let Some(data) = spot_data["data"].as_array() {
        let btc_spot = data.iter().any(|item| {
            item["instId"].as_str() == Some("BTC-USDT")
        });
        println!("  BTC-USDT in SPOT: {}", btc_spot);
    }

    // Check swap
    if let Some(data) = swap_data["data"].as_array() {
        let btc_swap = data.iter().any(|item| {
            item["instId"].as_str() == Some("BTC-USDT-SWAP")
        });
        let btc_usdt = data.iter().any(|item| {
            item["instId"].as_str() == Some("BTC-USDT")
        });
        println!("  BTC-USDT-SWAP in SWAP: {}", btc_swap);
        println!("  BTC-USDT in SWAP: {}", btc_usdt);
    }

    Ok(())
}