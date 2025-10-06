use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::{HashMap, VecDeque};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc, Local};

#[derive(Debug, Clone)]
struct PacketInfo {
    exchange: String,
    symbol: String,
    timestamp: i64,
    received_at: i64,
    packet_type: String,
    sequence: u64,
}

#[derive(Debug, Default)]
struct InversionStats {
    total_inversions: u64,
    small_inversions: u64,  // < 100ms
    medium_inversions: u64, // 100ms - 1s
    large_inversions: u64,  // > 1s
    max_inversion: i64,
    min_inversion: i64,
    exchange_inversions: HashMap<String, u64>,
    symbol_inversions: HashMap<String, u64>,
    hourly_pattern: [u64; 24],
    recent_inversions: VecDeque<(i64, String, i64)>, // (time, symbol, diff)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔬 PACKET ORDER PATTERN ANALYZER".bright_cyan().bold());
    println!("Analyzing ordering patterns to identify root causes...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001");
    println!("Analyzing packet ordering patterns...\n");
    println!("{}", "─".repeat(80).bright_blue());
    
    let mut buf = vec![0u8; 65536];
    let mut last_timestamps: HashMap<String, i64> = HashMap::new();
    let mut packet_sequences: HashMap<String, u64> = HashMap::new();
    let mut stats = InversionStats::default();
    stats.min_inversion = i64::MAX;
    
    let mut total_packets = 0u64;
    let mut packets_by_exchange: HashMap<String, u64> = HashMap::new();
    let mut packets_by_symbol: HashMap<String, u64> = HashMap::new();
    
    let start_time = Instant::now();
    let mut last_report = Instant::now();
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, _addr))) => {
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            total_packets += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown").to_string();
                            let symbol = json["symbol"].as_str().unwrap_or("unknown").to_string();
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            let key = format!("{}:{}", exchange, symbol);
                            
                            // Track packets by exchange and symbol
                            *packets_by_exchange.entry(exchange.clone()).or_insert(0) += 1;
                            *packets_by_symbol.entry(symbol.clone()).or_insert(0) += 1;
                            
                            // Track sequence
                            let seq = packet_sequences.entry(key.clone()).or_insert(0);
                            *seq += 1;
                            
                            // Get current time for analysis
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as i64;
                            
                            // Check for inversion
                            if let Some(&last_ts) = last_timestamps.get(&key) {
                                if timestamp < last_ts {
                                    let diff = last_ts - timestamp;
                                    
                                    // Update stats
                                    stats.total_inversions += 1;
                                    
                                    if diff < 100 {
                                        stats.small_inversions += 1;
                                    } else if diff < 1000 {
                                        stats.medium_inversions += 1;
                                    } else {
                                        stats.large_inversions += 1;
                                    }
                                    
                                    if diff > stats.max_inversion {
                                        stats.max_inversion = diff;
                                    }
                                    if diff < stats.min_inversion {
                                        stats.min_inversion = diff;
                                    }
                                    
                                    *stats.exchange_inversions.entry(exchange.clone()).or_insert(0) += 1;
                                    *stats.symbol_inversions.entry(symbol.clone()).or_insert(0) += 1;
                                    
                                    // Track hourly pattern
                                    let hour = Local::now().hour() as usize;
                                    stats.hourly_pattern[hour] += 1;
                                    
                                    // Keep recent inversions
                                    stats.recent_inversions.push_back((now, key.clone(), diff));
                                    if stats.recent_inversions.len() > 100 {
                                        stats.recent_inversions.pop_front();
                                    }
                                    
                                    // Real-time alert for significant inversions
                                    if diff > 1000 {
                                        println!("\n{} SIGNIFICANT INVERSION DETECTED!", "⚠️".bright_red());
                                        println!("   Symbol: {}", key.bright_yellow());
                                        println!("   Backward jump: {} ms", diff.to_string().bright_red());
                                        println!("   Previous: {} ({})", last_ts, format_timestamp(last_ts));
                                        println!("   Current:  {} ({})", timestamp, format_timestamp(timestamp));
                                        println!("   Network delay: {} ms", (now - timestamp).abs());
                                    }
                                }
                            }
                            
                            last_timestamps.insert(key, timestamp);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Error: {}", e);
            }
            Err(_) => {
                // Timeout - continue
            }
        }
        
        // Generate detailed report every 30 seconds
        if last_report.elapsed().as_secs() >= 30 {
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", "📊 PACKET ORDER PATTERN ANALYSIS REPORT".bright_cyan().bold());
            println!("{}", "═".repeat(80).bright_cyan());
            
            let elapsed = start_time.elapsed().as_secs();
            println!("\n⏱️  Runtime: {} seconds", elapsed);
            println!("📦 Total packets: {}", total_packets.to_string().bright_yellow());
            
            if stats.total_inversions > 0 {
                println!("\n{} INVERSION PATTERNS", "🔍".bright_white());
                println!("{}", "─".repeat(40).bright_blue());
                
                println!("Total inversions: {}", stats.total_inversions.to_string().bright_red());
                let inversion_rate = (stats.total_inversions as f64 / total_packets as f64) * 100.0;
                println!("Inversion rate: {:.3}%", inversion_rate);
                
                println!("\n📏 Inversion Size Distribution:");
                println!("   Small (<100ms):   {} ({:.1}%)", 
                    stats.small_inversions,
                    (stats.small_inversions as f64 / stats.total_inversions as f64) * 100.0
                );
                println!("   Medium (100-1000ms): {} ({:.1}%)",
                    stats.medium_inversions,
                    (stats.medium_inversions as f64 / stats.total_inversions as f64) * 100.0
                );
                println!("   Large (>1s):      {} ({:.1}%)",
                    stats.large_inversions,
                    (stats.large_inversions as f64 / stats.total_inversions as f64) * 100.0
                );
                
                println!("\n📊 Range:");
                println!("   Min inversion: {} ms", stats.min_inversion);
                println!("   Max inversion: {} ms", stats.max_inversion);
                
                // Exchange analysis
                if !stats.exchange_inversions.is_empty() {
                    println!("\n🏢 By Exchange:");
                    let mut sorted_exchanges: Vec<_> = stats.exchange_inversions.iter().collect();
                    sorted_exchanges.sort_by(|a, b| b.1.cmp(a.1));
                    
                    for (exchange, count) in sorted_exchanges.iter().take(5) {
                        let total = packets_by_exchange.get(*exchange).unwrap_or(&1);
                        let rate = (*count as f64 / *total as f64) * 100.0;
                        println!("   {} - {} inversions ({:.3}% of {} packets)",
                            exchange.bright_yellow(),
                            count.to_string().red(),
                            rate,
                            total
                        );
                    }
                }
                
                // Symbol analysis
                if !stats.symbol_inversions.is_empty() {
                    println!("\n💹 Top Symbols with Inversions:");
                    let mut sorted_symbols: Vec<_> = stats.symbol_inversions.iter().collect();
                    sorted_symbols.sort_by(|a, b| b.1.cmp(a.1));
                    
                    for (symbol, count) in sorted_symbols.iter().take(5) {
                        println!("   {} - {} inversions", 
                            symbol.bright_cyan(),
                            count.to_string().red()
                        );
                    }
                }
                
                // Time pattern analysis
                println!("\n🕐 Hourly Distribution:");
                let max_hour_count = *stats.hourly_pattern.iter().max().unwrap_or(&1);
                if max_hour_count > 0 {
                    for (hour, count) in stats.hourly_pattern.iter().enumerate() {
                        if *count > 0 {
                            let bar_len = (*count * 30 / max_hour_count) as usize;
                            let bar = "█".repeat(bar_len);
                            println!("   {:02}:00 {} {}", hour, bar.bright_blue(), count);
                        }
                    }
                }
                
                // Recent inversions
                if !stats.recent_inversions.is_empty() {
                    println!("\n📍 Recent Inversions (last 10):");
                    for (time, symbol, diff) in stats.recent_inversions.iter().rev().take(10) {
                        let dt = DateTime::<Local>::from(
                            SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(*time as u64)
                        );
                        println!("   {} - {} backward {} ms",
                            dt.format("%H:%M:%S"),
                            symbol.yellow(),
                            diff.to_string().red()
                        );
                    }
                }
                
                // Pattern detection
                println!("\n💡 PATTERN INSIGHTS:");
                if stats.small_inversions > stats.medium_inversions + stats.large_inversions {
                    println!("   ✓ Mostly small inversions - likely normal network jitter");
                }
                if stats.large_inversions > stats.total_inversions / 10 {
                    println!("   ⚠ Many large inversions - possible timestamp format issues");
                }
                
                // Check if specific exchanges have issues
                let mut exchange_pattern = false;
                for (exchange, count) in &stats.exchange_inversions {
                    let total = packets_by_exchange.get(exchange).unwrap_or(&1);
                    let rate = (*count as f64 / *total as f64) * 100.0;
                    if rate > 5.0 {
                        println!("   ⚠ {} has high inversion rate ({:.1}%)", exchange, rate);
                        exchange_pattern = true;
                    }
                }
                
                if !exchange_pattern && stats.total_inversions > 0 {
                    println!("   ✓ Inversions distributed across exchanges");
                }
                
            } else {
                println!("\n{} No inversions detected - all packets in order!", "✅".bright_green());
            }
            
            println!("\n{}", "═".repeat(80).bright_cyan());
            last_report = Instant::now();
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let seconds = ts / 1000;
    let millis = (ts % 1000) as u32;
    
    if let Some(dt) = chrono::DateTime::from_timestamp(seconds, millis * 1_000_000) {
        dt.format("%H:%M:%S.%3f").to_string()
    } else {
        format!("Invalid({})", ts)
    }
}