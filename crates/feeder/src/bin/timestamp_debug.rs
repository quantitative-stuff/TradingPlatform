use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔍 TIMESTAMP DEBUG TOOL".bright_cyan().bold());
    println!("Analyzing timestamp formats in packets...\n");
    
    // Connect to the correct multicast
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001\n");
    println!("Analyzing first 10 packets...\n");
    
    let mut buf = vec![0u8; 65536];
    let mut packet_count = 0;
    
    while packet_count < 10 {
        match socket.recv_from(&mut buf).await {
            Ok((len, _addr)) => {
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        packet_count += 1;
                        
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            let exchange = json["exchange"].as_str().unwrap_or("unknown");
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            println!("📦 Packet #{}", packet_count);
                            println!("   Exchange: {}", exchange.bright_yellow());
                            println!("   Symbol: {}", symbol.bright_cyan());
                            println!("   Raw timestamp: {}", timestamp.to_string().bright_green());
                            
                            // Analyze the timestamp
                            let timestamp_str = timestamp.to_string();
                            let digit_count = timestamp_str.len();
                            
                            println!("   Digit count: {}", digit_count);
                            
                            // Check different timestamp interpretations
                            if digit_count == 13 {
                                // Milliseconds since epoch
                                let seconds = timestamp / 1000;
                                let datetime = chrono::DateTime::from_timestamp(seconds, 0);
                                if let Some(dt) = datetime {
                                    println!("   Interpreted as: {} (milliseconds)", dt.to_rfc3339().bright_white());
                                }
                            } else if digit_count == 10 {
                                // Seconds since epoch
                                let datetime = chrono::DateTime::from_timestamp(timestamp, 0);
                                if let Some(dt) = datetime {
                                    println!("   Interpreted as: {} (seconds)", dt.to_rfc3339().bright_white());
                                }
                            } else if digit_count == 16 {
                                // Microseconds since epoch
                                let seconds = timestamp / 1_000_000;
                                let datetime = chrono::DateTime::from_timestamp(seconds, 0);
                                if let Some(dt) = datetime {
                                    println!("   Interpreted as: {} (microseconds)", dt.to_rfc3339().bright_white());
                                }
                            } else if digit_count < 11 {
                                // Might be a relative timestamp or sequence number
                                println!("   ⚠️  Unusual timestamp: {} digits - might be sequence number or relative time", digit_count);
                                
                                // Try to interpret as seconds anyway
                                let datetime = chrono::DateTime::from_timestamp(timestamp, 0);
                                if let Some(dt) = datetime {
                                    println!("   If seconds: {}", dt.to_rfc3339().yellow());
                                }
                            } else {
                                println!("   ❓ Unknown format with {} digits", digit_count);
                            }
                            
                            // Show current time for comparison
                            let now = chrono::Utc::now();
                            let now_millis = now.timestamp_millis();
                            println!("   Current time: {} ({})", now.to_rfc3339(), now_millis);
                            
                            // Check if it's way off
                            if digit_count == 13 {
                                let diff = (now_millis - timestamp).abs();
                                if diff > 86400000 * 365 {  // More than a year off
                                    println!("   {} Timestamp is more than a year off from current time!", "⚠️".red());
                                }
                            }
                            
                            println!("{}", "─".repeat(50));
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }
    
    println!("\n💡 Analysis complete!");
    println!("\nCommon timestamp formats:");
    println!("  10 digits = seconds since epoch (Unix timestamp)");
    println!("  13 digits = milliseconds since epoch (JavaScript/Java style)");
    println!("  16 digits = microseconds since epoch");
    println!("  19 digits = nanoseconds since epoch");
    
    Ok(())
}