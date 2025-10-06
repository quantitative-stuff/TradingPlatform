use std::net::UdpSocket;
use std::time::Duration;
use tokio::time::timeout;
use serde_json::Value;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 Bybit Debug Monitor - Checking UDP data feed");
    println!("📡 Listening on UDP multicast 239.1.1.1:9001");
    println!("⏰ Will run for 30 seconds to capture Bybit data");
    println!("========================================");

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    
    // Join multicast group
    let multicast_addr = std::net::Ipv4Addr::new(239, 1, 1, 1);
    let interface_addr = std::net::Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(&multicast_addr, &interface_addr)?;
    
    let mut buffer = [0; 8192];
    let mut bybit_trade_count = 0;
    let mut bybit_orderbook_count = 0;
    let mut bybit_connection_count = 0;
    let mut total_packets = 0;
    let mut unique_symbols = std::collections::HashSet::new();
    
    let start_time = std::time::Instant::now();
    let duration_limit = Duration::from_secs(30);
    
    while start_time.elapsed() < duration_limit {
        match socket.recv_from(&mut buffer) {
            Ok((size, _addr)) => {
                let data = &buffer[..size];
                
                if let Ok(text) = std::str::from_utf8(data) {
                    total_packets += 1;
                    
                    // Check if it's a connection status packet
                    if text.starts_with("CONN|") {
                        let parts: Vec<&str> = text.split('|').collect();
                        if parts.len() >= 2 && parts[1].to_lowercase().contains("bybit") {
                            bybit_connection_count += 1;
                            println!("🔗 Connection: {}", text);
                        }
                        continue;
                    }
                    
                    // Try to parse as JSON for trade/orderbook data
                    if let Ok(json) = serde_json::from_str::<Value>(text) {
                        if let Some(exchange) = json.get("exchange").and_then(Value::as_str) {
                            if exchange == "Bybit" {
                                if let Some(symbol) = json.get("symbol").and_then(Value::as_str) {
                                    unique_symbols.insert(symbol.to_string());
                                }
                                
                                // Check data type
                                if json.get("price").is_some() && json.get("quantity").is_some() {
                                    // This is trade data
                                    bybit_trade_count += 1;
                                    if bybit_trade_count <= 5 {
                                        println!("💰 Trade: {} - {} @ {} (qty: {})", 
                                            json.get("symbol").and_then(Value::as_str).unwrap_or("?"),
                                            json.get("asset_type").and_then(Value::as_str).unwrap_or("?"),
                                            json.get("price").and_then(Value::as_f64).unwrap_or(0.0),
                                            json.get("quantity").and_then(Value::as_f64).unwrap_or(0.0)
                                        );
                                    }
                                } else if json.get("bids").is_some() || json.get("asks").is_some() {
                                    // This is orderbook data
                                    bybit_orderbook_count += 1;
                                    if bybit_orderbook_count <= 5 {
                                        let bids_len = json.get("bids").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                        let asks_len = json.get("asks").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                        println!("📊 OrderBook: {} - {} (bids: {}, asks: {})", 
                                            json.get("symbol").and_then(Value::as_str).unwrap_or("?"),
                                            json.get("asset_type").and_then(Value::as_str).unwrap_or("?"),
                                            bids_len,
                                            asks_len
                                        );
                                    }
                                }
                            }
                        }
                    }
                    
                    // Print progress every 5 seconds
                    if start_time.elapsed().as_secs() % 5 == 0 && total_packets > 0 {
                        println!("📈 Progress: {}s - Trades: {}, OrderBooks: {}, Connections: {}, Unique Symbols: {}", 
                            start_time.elapsed().as_secs(),
                            bybit_trade_count, 
                            bybit_orderbook_count,
                            bybit_connection_count,
                            unique_symbols.len()
                        );
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout, continue
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => {
                eprintln!("❌ Socket error: {}", e);
                break;
            }
        }
    }
    
    println!("\n========================================");
    println!("📊 FINAL BYBIT DEBUG RESULTS:");
    println!("⏱️  Total runtime: {:.1}s", start_time.elapsed().as_secs_f64());
    println!("📦 Total packets received: {}", total_packets);
    println!("💰 Bybit trades received: {}", bybit_trade_count);
    println!("📊 Bybit orderbooks received: {}", bybit_orderbook_count);
    println!("🔗 Bybit connections seen: {}", bybit_connection_count);
    println!("🎯 Unique Bybit symbols: {}", unique_symbols.len());
    
    if unique_symbols.len() > 0 {
        println!("\n🎯 Sample symbols seen:");
        for (i, symbol) in unique_symbols.iter().take(10).enumerate() {
            print!("{}", symbol);
            if i < unique_symbols.len().min(10) - 1 {
                print!(", ");
            }
        }
        if unique_symbols.len() > 10 {
            println!("... and {} more", unique_symbols.len() - 10);
        } else {
            println!();
        }
    }
    
    println!("\n🔍 DIAGNOSIS:");
    if bybit_connection_count == 0 {
        println!("❌ No Bybit connection packets seen - feeder may not be running");
    } else if bybit_trade_count == 0 && bybit_orderbook_count == 0 {
        println!("⚠️  Bybit connections seen but no market data - subscription issue");
    } else if unique_symbols.len() < 10 {
        println!("⚠️  Very few symbols receiving data - possible symbol filtering issue");
    } else {
        println!("✅ Bybit appears to be working - receiving market data");
    }
    
    Ok(())
}