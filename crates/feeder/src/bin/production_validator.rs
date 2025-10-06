use tokio::net::UdpSocket;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Instant, Duration};
use colored::*;
use serde_json;
use std::net::Ipv4Addr;

/// This validator monitors REAL production data without any artificial errors
/// It will show you ACTUAL ordering issues if they exist

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔍 PRODUCTION ORDER VALIDATOR".bright_green().bold());
    println!("Monitoring real packet flow for ordering issues...\n");
    println!("NO artificial errors - only detecting REAL issues\n");
    
    // Bind to multicast to receive real production packets
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;  // CORRECT ADDRESS FROM FEEDER_CONFIG
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("✓ Connected to production multicast stream");
    println!("✓ Monitoring packets on 239.1.1.1:9001\n");
    
    // Track last timestamp for each exchange:symbol
    let last_timestamps: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));
    let inversion_counts: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    let packet_counts: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    
    let mut buf = vec![0u8; 65536];
    let mut total_packets = 0u64;
    let mut last_report = Instant::now();
    let mut last_stats_update = Instant::now();
    let mut packet_types: HashMap<String, u64> = HashMap::new();
    
    println!("{}", "Starting to monitor... (real issues will appear below)".yellow());
    println!("{}", "─".repeat(70).bright_blue());
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, addr))) => {
                total_packets += 1;
                
                // Show first few packets for debugging
                if total_packets <= 5 {
                    println!("📦 Packet #{} from {} ({} bytes)", 
                        total_packets, addr, len);
                }
                
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 {
                        let packet_type = parts[0];
                        *packet_types.entry(packet_type.to_string()).or_insert(0) += 1;
                        
                        // Parse trade or orderbook data
                        let (exchange, symbol, timestamp) = match packet_type {
                            "TRADE" | "ORDERBOOK" => {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                                    let exchange = json["exchange"].as_str().unwrap_or("unknown");
                                    let symbol = json["symbol"].as_str().unwrap_or("unknown");
                                    let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                                    (exchange.to_string(), symbol.to_string(), timestamp)
                                } else {
                                    continue;
                                }
                            }
                            _ => continue,
                        };
                        
                        let key = format!("{}:{}", exchange, symbol);
                        
                        // Check for timestamp inversion (REAL issue detection)
                        let mut timestamps = last_timestamps.lock().await;
                        let mut inversions = inversion_counts.lock().await;
                        let mut counts = packet_counts.lock().await;
                        
                        *counts.entry(key.clone()).or_insert(0) += 1;
                        total_packets += 1;
                        
                        if let Some(&last_ts) = timestamps.get(&key) {
                            if timestamp < last_ts {
                                // REAL INVERSION DETECTED!
                                *inversions.entry(key.clone()).or_insert(0) += 1;
                                
                                let inversion_ms = last_ts - timestamp;
                                
                                // Report significant inversions immediately
                                println!(
                                    "⚠️  {} REAL INVERSION: {} went backwards by {}ms ({}→{})",
                                    chrono::Local::now().format("%H:%M:%S").to_string().bright_yellow(),
                                    key.bright_red(),
                                    inversion_ms.to_string().bright_red(),
                                    last_ts,
                                    timestamp
                                );
                            }
                        }
                        
                        timestamps.insert(key, timestamp);
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Error receiving packet: {}", e);
            }
            Err(_) => {
                // Timeout - no packet received in 1 second
                println!("⏳ No packets received in the last second... (Total: {})", total_packets);
                
                if total_packets == 0 {
                    println!("   Check if:");
                    println!("   1. Feeder is running: cargo run --bin feeder");
                    println!("   2. Multicast address is correct: 239.1.1.1:9001");
                    println!("   3. Firewall allows UDP traffic");
                }
            }
        }
        
        // Show live stats every 5 seconds
        if last_stats_update.elapsed().as_secs() >= 5 {
            let counts = packet_counts.lock().await;
            let inversions = inversion_counts.lock().await;
            
            // Clear screen and show live stats
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", "📊 PRODUCTION ORDER VALIDATOR - LIVE STATS".bright_cyan().bold());
            println!("{}", "═".repeat(70).bright_cyan());
            println!();
            
            println!("📡 Connection: {}", "239.1.1.1:9001".bright_green());
            println!("📦 Total Packets: {}", total_packets.to_string().bright_yellow());
            println!("⏱️  Uptime: {}s", last_report.elapsed().as_secs());
            println!();
            
            if !packet_types.is_empty() {
                println!("📨 Packet Types:");
                for (ptype, count) in &packet_types {
                    println!("   {} {}: {}", 
                        if ptype == "TRADE" { "💹" } else if ptype == "ORDERBOOK" { "📊" } else { "📄" },
                        ptype.bright_white(), 
                        count.to_string().cyan()
                    );
                }
                println!();
            }
            
            if !counts.is_empty() {
                println!("🏢 Active Symbols ({} total):", counts.len());
                let mut sorted: Vec<_> = counts.iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(a.1));
                
                for (key, count) in sorted.iter().take(10) {
                    let inv_count = inversions.get(*key).unwrap_or(&0);
                    if *inv_count > 0 {
                        let percentage = (*inv_count as f64 / **count as f64) * 100.0;
                        println!("   {} - {} packets, {} inversions ({:.2}%)",
                            key.bright_white(),
                            count.to_string().cyan(),
                            inv_count.to_string().red(),
                            percentage
                        );
                    } else {
                        println!("   {} - {} packets ✅",
                            key.bright_white(),
                            count.to_string().cyan()
                        );
                    }
                }
                
                if counts.len() > 10 {
                    println!("   ... and {} more", counts.len() - 10);
                }
            } else {
                println!("⏳ Waiting for data...");
            }
            
            println!();
            println!("{}", "─".repeat(70).bright_blue());
            
            if !inversions.is_empty() {
                println!("{} ORDERING ISSUES DETECTED:", "⚠️".bright_yellow());
                for (key, inv_count) in inversions.iter() {
                    println!("   {} has {} inversions", key.bright_red(), inv_count);
                }
            } else if total_packets > 0 {
                println!("{} All packets in correct order!", "✅".bright_green());
            }
            
            last_stats_update = Instant::now();
        }
        
        // Print detailed report every 30 seconds
        if last_report.elapsed().as_secs() >= 30 {
            let inversions = inversion_counts.lock().await;
            let counts = packet_counts.lock().await;
            
            println!("\n{}", "═".repeat(70).bright_cyan());
            println!("{} 30-SECOND REPORT", "📊".bright_white());
            println!("{}", "─".repeat(70).bright_blue());
            
            if inversions.is_empty() {
                println!("{}", "✅ NO ORDERING ISSUES DETECTED!".bright_green().bold());
                println!("   All {} packets arrived in correct order", total_packets);
            } else {
                println!("{}", "⚠️  ORDERING ISSUES FOUND:".bright_yellow().bold());
                
                for (key, inv_count) in inversions.iter() {
                    if let Some(total) = counts.get(key) {
                        let percentage = (*inv_count as f64 / *total as f64) * 100.0;
                        println!(
                            "   {} - {} inversions out of {} packets ({:.3}%)",
                            key.bright_white(),
                            inv_count.to_string().bright_red(),
                            total,
                            percentage
                        );
                    }
                }
            }
            
            println!("\nTotal packets processed: {}", total_packets.to_string().bright_cyan());
            println!("{}", "═".repeat(70).bright_cyan());
            
            last_report = Instant::now();
        }
    }
}