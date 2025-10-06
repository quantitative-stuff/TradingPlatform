use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
struct SymbolInfo {
    symbol: String,
    base_asset: String,
    quote_asset: String,
    market_type: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExchangeSymbols {
    exchange: String,
    spot: Vec<SymbolInfo>,
    futures: Vec<SymbolInfo>,
    total_spot: usize,
    total_futures: usize,
}

async fn fetch_binance_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching Binance symbols...");
    
    // Fetch spot symbols
    let spot_url = "https://api.binance.com/api/v3/exchangeInfo";
    let spot_response = reqwest::get(spot_url).await?;
    let spot_data: Value = spot_response.json().await?;
    
    let mut spot_symbols = Vec::new();
    if let Some(symbols) = spot_data["symbols"].as_array() {
        for symbol in symbols {
            if symbol["status"].as_str() == Some("TRADING") {
                spot_symbols.push(SymbolInfo {
                    symbol: symbol["symbol"].as_str().unwrap_or("").to_string(),
                    base_asset: symbol["baseAsset"].as_str().unwrap_or("").to_string(),
                    quote_asset: symbol["quoteAsset"].as_str().unwrap_or("").to_string(),
                    market_type: "spot".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    // Fetch futures symbols
    let futures_url = "https://fapi.binance.com/fapi/v1/exchangeInfo";
    let futures_response = reqwest::get(futures_url).await?;
    let futures_data: Value = futures_response.json().await?;
    
    let mut futures_symbols = Vec::new();
    if let Some(symbols) = futures_data["symbols"].as_array() {
        for symbol in symbols {
            if symbol["status"].as_str() == Some("TRADING") {
                futures_symbols.push(SymbolInfo {
                    symbol: symbol["symbol"].as_str().unwrap_or("").to_string(),
                    base_asset: symbol["baseAsset"].as_str().unwrap_or("").to_string(),
                    quote_asset: symbol["quoteAsset"].as_str().unwrap_or("").to_string(),
                    market_type: symbol["contractType"].as_str().unwrap_or("PERPETUAL").to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("Binance - Spot: {}, Futures: {}", spot_symbols.len(), futures_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "Binance".to_string(),
        total_spot: spot_symbols.len(),
        total_futures: futures_symbols.len(),
        spot: spot_symbols,
        futures: futures_symbols,
    })
}

async fn fetch_bybit_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching Bybit symbols...");
    
    let mut spot_symbols = Vec::new();
    let mut futures_symbols = Vec::new();
    
    // Fetch all instruments info
    let url = "https://api.bybit.com/v5/market/instruments-info";
    
    // Fetch spot
    let spot_url = format!("{}?category=spot", url);
    let spot_response = reqwest::get(&spot_url).await?;
    let spot_data: Value = spot_response.json().await?;
    
    if let Some(list) = spot_data["result"]["list"].as_array() {
        for item in list {
            if item["status"].as_str() == Some("Trading") {
                spot_symbols.push(SymbolInfo {
                    symbol: item["symbol"].as_str().unwrap_or("").to_string(),
                    base_asset: item["baseCoin"].as_str().unwrap_or("").to_string(),
                    quote_asset: item["quoteCoin"].as_str().unwrap_or("").to_string(),
                    market_type: "spot".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    // Fetch linear futures
    let linear_url = format!("{}?category=linear", url);
    let linear_response = reqwest::get(&linear_url).await?;
    let linear_data: Value = linear_response.json().await?;
    
    if let Some(list) = linear_data["result"]["list"].as_array() {
        for item in list {
            if item["status"].as_str() == Some("Trading") {
                futures_symbols.push(SymbolInfo {
                    symbol: item["symbol"].as_str().unwrap_or("").to_string(),
                    base_asset: item["baseCoin"].as_str().unwrap_or("").to_string(),
                    quote_asset: item["quoteCoin"].as_str().unwrap_or("").to_string(),
                    market_type: "linear".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("Bybit - Spot: {}, Futures: {}", spot_symbols.len(), futures_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "Bybit".to_string(),
        total_spot: spot_symbols.len(),
        total_futures: futures_symbols.len(),
        spot: spot_symbols,
        futures: futures_symbols,
    })
}

async fn fetch_upbit_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching Upbit symbols...");
    
    let url = "https://api.upbit.com/v1/market/all";
    let response = reqwest::get(url).await?;
    let data: Value = response.json().await?;
    
    let mut spot_symbols = Vec::new();
    
    if let Some(markets) = data.as_array() {
        for market in markets {
            let market_code = market["market"].as_str().unwrap_or("");
            let parts: Vec<&str> = market_code.split('-').collect();
            
            if parts.len() == 2 {
                spot_symbols.push(SymbolInfo {
                    symbol: market_code.to_string(),
                    base_asset: parts[1].to_string(),
                    quote_asset: parts[0].to_string(),
                    market_type: "spot".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("Upbit - Spot: {}, Futures: 0 (Upbit doesn't have futures)", spot_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "Upbit".to_string(),
        total_spot: spot_symbols.len(),
        total_futures: 0,
        spot: spot_symbols,
        futures: vec![],
    })
}

async fn fetch_coinbase_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching Coinbase symbols...");
    
    let url = "https://api.exchange.coinbase.com/products";
    let response = reqwest::get(url).await?;
    let data: Value = response.json().await?;
    
    let mut spot_symbols = Vec::new();
    
    if let Some(products) = data.as_array() {
        for product in products {
            if product["status"].as_str() == Some("online") {
                spot_symbols.push(SymbolInfo {
                    symbol: product["id"].as_str().unwrap_or("").to_string(),
                    base_asset: product["base_currency"].as_str().unwrap_or("").to_string(),
                    quote_asset: product["quote_currency"].as_str().unwrap_or("").to_string(),
                    market_type: "spot".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("Coinbase - Spot: {}, Futures: 0 (Coinbase doesn't have futures)", spot_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "Coinbase".to_string(),
        total_spot: spot_symbols.len(),
        total_futures: 0,
        spot: spot_symbols,
        futures: vec![],
    })
}

async fn fetch_okx_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching OKX symbols...");
    
    let mut spot_symbols = Vec::new();
    let mut futures_symbols = Vec::new();
    
    // Fetch spot symbols
    let spot_url = "https://www.okx.com/api/v5/public/instruments?instType=SPOT";
    let spot_response = reqwest::get(spot_url).await?;
    let spot_data: Value = spot_response.json().await?;
    
    if let Some(data) = spot_data["data"].as_array() {
        for item in data {
            if item["state"].as_str() == Some("live") {
                spot_symbols.push(SymbolInfo {
                    symbol: item["instId"].as_str().unwrap_or("").to_string(),
                    base_asset: item["baseCcy"].as_str().unwrap_or("").to_string(),
                    quote_asset: item["quoteCcy"].as_str().unwrap_or("").to_string(),
                    market_type: "spot".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    // Fetch swap (perpetual futures) symbols
    let swap_url = "https://www.okx.com/api/v5/public/instruments?instType=SWAP";
    let swap_response = reqwest::get(swap_url).await?;
    let swap_data: Value = swap_response.json().await?;
    
    if let Some(data) = swap_data["data"].as_array() {
        for item in data {
            if item["state"].as_str() == Some("live") {
                futures_symbols.push(SymbolInfo {
                    symbol: item["instId"].as_str().unwrap_or("").to_string(),
                    base_asset: item["ctValCcy"].as_str().unwrap_or("").to_string(),
                    quote_asset: item["settleCcy"].as_str().unwrap_or("").to_string(),
                    market_type: "swap".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    // Fetch futures symbols
    let futures_url = "https://www.okx.com/api/v5/public/instruments?instType=FUTURES";
    let futures_response = reqwest::get(futures_url).await?;
    let futures_data: Value = futures_response.json().await?;
    
    if let Some(data) = futures_data["data"].as_array() {
        for item in data {
            if item["state"].as_str() == Some("live") {
                futures_symbols.push(SymbolInfo {
                    symbol: item["instId"].as_str().unwrap_or("").to_string(),
                    base_asset: item["ctValCcy"].as_str().unwrap_or("").to_string(),
                    quote_asset: item["settleCcy"].as_str().unwrap_or("").to_string(),
                    market_type: "futures".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("OKX - Spot: {}, Futures: {}", spot_symbols.len(), futures_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "OKX".to_string(),
        total_spot: spot_symbols.len(),
        total_futures: futures_symbols.len(),
        spot: spot_symbols,
        futures: futures_symbols,
    })
}

async fn fetch_deribit_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching Deribit symbols...");
    
    let mut futures_symbols = Vec::new();
    
    // Fetch BTC instruments
    let btc_url = "https://www.deribit.com/api/v2/public/get_instruments?currency=BTC";
    let btc_response = reqwest::get(btc_url).await?;
    let btc_data: Value = btc_response.json().await?;
    
    if let Some(result) = btc_data["result"].as_array() {
        for item in result {
            if item["is_active"].as_bool() == Some(true) {
                futures_symbols.push(SymbolInfo {
                    symbol: item["instrument_name"].as_str().unwrap_or("").to_string(),
                    base_asset: "BTC".to_string(),
                    quote_asset: "USD".to_string(),
                    market_type: item["kind"].as_str().unwrap_or("").to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    // Fetch ETH instruments
    let eth_url = "https://www.deribit.com/api/v2/public/get_instruments?currency=ETH";
    let eth_response = reqwest::get(eth_url).await?;
    let eth_data: Value = eth_response.json().await?;
    
    if let Some(result) = eth_data["result"].as_array() {
        for item in result {
            if item["is_active"].as_bool() == Some(true) {
                futures_symbols.push(SymbolInfo {
                    symbol: item["instrument_name"].as_str().unwrap_or("").to_string(),
                    base_asset: "ETH".to_string(),
                    quote_asset: "USD".to_string(),
                    market_type: item["kind"].as_str().unwrap_or("").to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("Deribit - Spot: 0 (No spot trading), Futures: {}", futures_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "Deribit".to_string(),
        total_spot: 0,
        total_futures: futures_symbols.len(),
        spot: vec![],
        futures: futures_symbols,
    })
}

async fn fetch_bithumb_symbols() -> Result<ExchangeSymbols> {
    println!("Fetching Bithumb symbols...");
    
    let url = "https://api.bithumb.com/public/ticker/ALL_KRW";
    let response = reqwest::get(url).await?;
    let data: Value = response.json().await?;
    
    let mut spot_symbols = Vec::new();
    
    if let Some(data_obj) = data["data"].as_object() {
        for (symbol, _info) in data_obj {
            // Skip the "date" field
            if symbol != "date" {
                spot_symbols.push(SymbolInfo {
                    symbol: format!("{}_KRW", symbol),
                    base_asset: symbol.to_string(),
                    quote_asset: "KRW".to_string(),
                    market_type: "spot".to_string(),
                    status: "active".to_string(),
                });
            }
        }
    }
    
    println!("Bithumb - Spot: {}, Futures: 0 (Bithumb doesn't have futures)", spot_symbols.len());
    
    Ok(ExchangeSymbols {
        exchange: "Bithumb".to_string(),
        total_spot: spot_symbols.len(),
        total_futures: 0,
        spot: spot_symbols,
        futures: vec![],
    })
}

fn generate_config_files(exchanges: &[ExchangeSymbols]) -> Result<()> {
    let config_dir = Path::new("src/config");
    
    for exchange in exchanges {
        let exchange_lower = exchange.exchange.to_lowercase();
        
        // Filter USDT pairs for spot and futures
        let spot_usdt: Vec<String> = exchange.spot.iter()
            .filter(|s| s.quote_asset == "USDT" || s.quote_asset == "USD")
            .map(|s| s.symbol.clone())
            .collect();
            
        let futures_usdt: Vec<String> = exchange.futures.iter()
            .filter(|s| s.quote_asset == "USDT" || s.quote_asset == "USD")
            .map(|s| s.symbol.clone())
            .collect();
        
        // For Upbit and Bithumb, also include KRW pairs
        let krw_pairs: Vec<String> = if exchange.exchange == "Upbit" || exchange.exchange == "Bithumb" {
            exchange.spot.iter()
                .filter(|s| s.quote_asset == "KRW")
                .map(|s| s.symbol.clone())
                .collect()
        } else {
            vec![]
        };
        
        // Create updated config with connect_config
        let connect_config = match exchange_lower.as_str() {
            "binance" => json!({
                "ws_url": "wss://stream.binance.com:9443/ws",
                "futures_ws_url": "wss://fstream.binance.com/ws",
                "testnet_url": "https://testnet.binancefuture.com",
                "mainnet_url": "https://fapi.binance.com",
                "use_testnet": false
            }),
            "bybit" => json!({
                "ws_url": "wss://stream.bybit.com/v5/public/spot",
                "futures_ws_url": "wss://stream.bybit.com/v5/public/linear",
                "testnet_url": "https://api-testnet.bybit.com",
                "mainnet_url": "https://api.bybit.com",
                "use_testnet": false
            }),
            "upbit" => json!({
                "ws_url": "wss://api.upbit.com/websocket/v1",
                "futures_ws_url": "",
                "testnet_url": "",
                "mainnet_url": "https://api.upbit.com",
                "use_testnet": false
            }),
            _ => json!({
                "ws_url": "",
                "futures_ws_url": "",
                "testnet_url": "",
                "mainnet_url": "",
                "use_testnet": false
            })
        };
        
        // Create updated config
        let config = json!({
            "output_dir": format!("data/{}", exchange_lower),
            "feed_config": {
                "exchange": exchange_lower,
                "asset_type": if exchange.futures.is_empty() {
                    vec!["spot"]
                } else {
                    match exchange_lower.as_str() {
                        "binance" => vec!["spot", "futures"],
                        "bybit" => vec!["spot", "linear"],
                        "okx" => vec!["spot", "swap"],
                        "deribit" => vec!["futures", "options"],
                        _ => vec!["spot", "futures"]
                    }
                },
                "max_symbols_per_connection": 5,
                "connection_timeout_seconds": 30,
                "reconnect_delay_seconds": 5,
                "max_reconnect_attempts": 10
            },
            "subscribe_data": {
                "codes": if exchange.exchange == "Bithumb" {
                    // For Bithumb, use KRW pairs as main symbols
                    if krw_pairs.len() > 100 {
                        krw_pairs[0..100].to_vec()
                    } else {
                        krw_pairs.clone()
                    }
                } else if spot_usdt.len() > 100 {
                    spot_usdt[0..100].to_vec()
                } else {
                    spot_usdt.clone()
                },
                "spot_symbols": if exchange.exchange == "Bithumb" {
                    krw_pairs.clone()
                } else {
                    spot_usdt.clone()
                },
                "futures_symbols": futures_usdt,
                "krw_symbols": krw_pairs,
                "stream_type": ["trade", "orderbook"],
                "order_depth": 20
            },
            "connect_config": connect_config,
            "trading_config": {
                "max_position": 0.01,
                "min_spread": 10.0,
                "tick_size": 0.01
            },
            "statistics": {
                "total_spot_available": exchange.total_spot,
                "total_futures_available": exchange.total_futures,
                "spot_usdt_count": spot_usdt.len(),
                "futures_usdt_count": futures_usdt.len(),
                "krw_count": krw_pairs.len()
            }
        });
        
        // Save config
        let config_path = config_dir.join(format!("{}_config_full.json", exchange_lower));
        fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
        println!("Saved config to: {:?}", config_path);
    }
    
    Ok(())
}

fn generate_summary_report(exchanges: &[ExchangeSymbols]) -> Result<()> {
    let mut report = String::new();
    report.push_str("# Exchange Symbol Summary Report\n\n");
    report.push_str(&format!("Generated: {}\n\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    
    // Overall statistics
    report.push_str("## Overall Statistics\n\n");
    report.push_str("| Exchange | Spot Symbols | Futures Symbols | Total |\n");
    report.push_str("|----------|-------------|-----------------|-------|\n");
    
    let mut total_spot = 0;
    let mut total_futures = 0;
    
    for exchange in exchanges {
        report.push_str(&format!("| {} | {} | {} | {} |\n", 
            exchange.exchange,
            exchange.total_spot,
            exchange.total_futures,
            exchange.total_spot + exchange.total_futures
        ));
        total_spot += exchange.total_spot;
        total_futures += exchange.total_futures;
    }
    
    report.push_str(&format!("| **Total** | **{}** | **{}** | **{}** |\n\n", 
        total_spot, total_futures, total_spot + total_futures));
    
    // Quote asset distribution
    report.push_str("## Quote Asset Distribution\n\n");
    
    for exchange in exchanges {
        report.push_str(&format!("### {}\n\n", exchange.exchange));
        
        // Count by quote asset
        let mut quote_counts: HashMap<String, usize> = HashMap::new();
        for symbol in &exchange.spot {
            *quote_counts.entry(symbol.quote_asset.clone()).or_insert(0) += 1;
        }
        for symbol in &exchange.futures {
            *quote_counts.entry(format!("{}_futures", symbol.quote_asset)).or_insert(0) += 1;
        }
        
        report.push_str("| Quote Asset | Count |\n");
        report.push_str("|-------------|-------|\n");
        
        let mut sorted_quotes: Vec<_> = quote_counts.iter().collect();
        sorted_quotes.sort_by(|a, b| b.1.cmp(a.1));
        
        for (quote, count) in sorted_quotes.iter().take(10) {
            report.push_str(&format!("| {} | {} |\n", quote, count));
        }
        report.push_str("\n");
    }
    
    // Common symbols across exchanges
    report.push_str("## Common Trading Pairs\n\n");
    
    let mut symbol_exchanges: HashMap<String, HashSet<String>> = HashMap::new();
    
    for exchange in exchanges {
        for symbol in &exchange.spot {
            if symbol.quote_asset == "USDT" || symbol.quote_asset == "USD" {
                let normalized = format!("{}^{}", symbol.base_asset, symbol.quote_asset);
                symbol_exchanges.entry(normalized).or_insert_with(HashSet::new).insert(exchange.exchange.clone());
            }
        }
    }
    
    let mut common_symbols: Vec<_> = symbol_exchanges.iter()
        .filter(|(_, exchanges)| exchanges.len() >= 2)
        .collect();
    common_symbols.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    
    report.push_str("| Symbol | Available On |\n");
    report.push_str("|--------|-------------|\n");
    
    for (symbol, exchanges) in common_symbols.iter().take(50) {
        let exchange_list: Vec<String> = exchanges.iter().map(|s| s.to_string()).collect();
        report.push_str(&format!("| {} | {} |\n", symbol, exchange_list.join(", ")));
    }
    
    // Save report
    fs::write("exchange_symbols_report.md", report)?;
    println!("\nSaved summary report to: exchange_symbols_report.md");
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Fetching All Crypto Assets from Exchanges ===\n");
    
    let mut exchanges = Vec::new();
    
    // Fetch from each exchange
    match fetch_binance_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching Binance symbols: {}", e),
    }
    
    match fetch_bybit_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching Bybit symbols: {}", e),
    }
    
    match fetch_upbit_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching Upbit symbols: {}", e),
    }
    
    match fetch_coinbase_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching Coinbase symbols: {}", e),
    }
    
    match fetch_okx_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching OKX symbols: {}", e),
    }
    
    match fetch_deribit_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching Deribit symbols: {}", e),
    }
    
    match fetch_bithumb_symbols().await {
        Ok(symbols) => exchanges.push(symbols),
        Err(e) => eprintln!("Error fetching Bithumb symbols: {}", e),
    }
    
    if exchanges.is_empty() {
        eprintln!("Failed to fetch symbols from any exchange");
        return Ok(());
    }
    
    // Generate config files
    println!("\n=== Generating Config Files ===");
    generate_config_files(&exchanges)?;
    
    // Generate summary report
    println!("\n=== Generating Summary Report ===");
    generate_summary_report(&exchanges)?;
    
    // Save raw data for reference
    let raw_data_path = "data/all_exchange_symbols.json";
    fs::write(raw_data_path, serde_json::to_string_pretty(&exchanges)?)?;
    println!("\nSaved raw data to: {}", raw_data_path);
    
    println!("\n=== Complete ===");
    Ok(())
}