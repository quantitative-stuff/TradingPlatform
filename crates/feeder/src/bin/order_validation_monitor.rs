use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::time::Instant;

/// Simplified tool that monitors ordering at receive side
/// and infers source issues based on patterns

#[derive(Debug, Default)]
struct ValidationStats {
    total_packets: u64,
    inversions: u64,
    
    // Pattern detection
    consecutive_inversions: u64,
    max_consecutive: u64,
    isolated_inversions: u64,
    
    // By exchange
    exchange_inversions: HashMap<String, u64>,
    exchange_packets: HashMap<String, u64>,
    
    // Inversion sizes
    tiny_inversions: u64,      // < 10ms
    small_inversions: u64,     // 10-100ms  
    medium_inversions: u64,    // 100-1000ms
    large_inversions: u64,     // > 1000ms
    
    last_timestamps: HashMap<String, i64>,
    consecutive_counter: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔍 ORDER VALIDATION MONITOR".bright_cyan().bold());
    println!("Analyzing packet ordering to determine source vs network issues...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001");
    println!("Monitoring packet order...\n");
    println!("{}", "─".repeat(80).bright_blue());
    
    let mut buf = vec![0u8; 65536];
    let mut stats = ValidationStats::default();
    let start_time = Instant::now();
    let mut last_report = Instant::now();
    let mut last_inversion_key = String::new();
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, _addr))) => {
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            stats.total_packets += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown").to_string();
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            let key = format!("{}:{}", exchange, symbol);
                            
                            *stats.exchange_packets.entry(exchange.clone()).or_insert(0) += 1;
                            
                            // Check for inversion
                            if let Some(&last_ts) = stats.last_timestamps.get(&key) {
                                if timestamp < last_ts {
                                    stats.inversions += 1;
                                    *stats.exchange_inversions.entry(exchange.clone()).or_insert(0) += 1;
                                    
                                    let diff = last_ts - timestamp;
                                    
                                    // Categorize by size
                                    if diff < 10 {
                                        stats.tiny_inversions += 1;
                                    } else if diff < 100 {
                                        stats.small_inversions += 1;
                                    } else if diff < 1000 {
                                        stats.medium_inversions += 1;
                                    } else {
                                        stats.large_inversions += 1;
                                    }
                                    
                                    // Track consecutive inversions
                                    if last_inversion_key == key {
                                        stats.consecutive_inversions += 1;
                                        stats.consecutive_counter += 1;
                                        if stats.consecutive_counter > stats.max_consecutive {
                                            stats.max_consecutive = stats.consecutive_counter;
                                        }
                                    } else {
                                        if stats.consecutive_counter == 0 {
                                            stats.isolated_inversions += 1;
                                        }
                                        stats.consecutive_counter = 0;
                                    }
                                    
                                    last_inversion_key = key.clone();
                                    
                                    // Real-time alert for large inversions
                                    if diff > 1000 {
                                        println!("{} Large inversion: {} back {}ms", 
                                            "⚠️".red(), 
                                            key.yellow(), 
                                            diff.to_string().red()
                                        );
                                    }
                                } else {
                                    // Reset consecutive counter if no inversion
                                    if last_inversion_key == key {
                                        stats.consecutive_counter = 0;
                                    }
                                }
                            }
                            
                            stats.last_timestamps.insert(key, timestamp);
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
        
        // Report every 15 seconds
        if last_report.elapsed().as_secs() >= 15 {
            print_validation_report(&stats, start_time.elapsed().as_secs());
            last_report = Instant::now();
        }
    }
}

fn print_validation_report(stats: &ValidationStats, elapsed: u64) {
    print!("\x1B[2J\x1B[1;1H");
    println!("{}", "📊 ORDER VALIDATION REPORT".bright_cyan().bold());
    println!("{}", "═".repeat(80).bright_cyan());
    
    println!("\n⏱️  Runtime: {} seconds", elapsed);
    println!("📦 Total packets: {}", stats.total_packets);
    
    if stats.inversions == 0 {
        println!("\n{} Perfect ordering - no inversions detected!", "✅".bright_green());
        return;
    }
    
    println!("\n⚠️  Total inversions: {}", stats.inversions.to_string().red());
    let rate = (stats.inversions as f64 / stats.total_packets as f64) * 100.0;
    println!("📈 Inversion rate: {:.4}%", rate);
    
    // Size distribution
    println!("\n📏 Inversion Size Distribution:");
    println!("   Tiny (<10ms):     {} ({:.1}%)", 
        stats.tiny_inversions,
        (stats.tiny_inversions as f64 / stats.inversions as f64) * 100.0
    );
    println!("   Small (10-100ms): {} ({:.1}%)",
        stats.small_inversions,
        (stats.small_inversions as f64 / stats.inversions as f64) * 100.0
    );
    println!("   Medium (100-1s):  {} ({:.1}%)",
        stats.medium_inversions,
        (stats.medium_inversions as f64 / stats.inversions as f64) * 100.0
    );
    println!("   Large (>1s):      {} ({:.1}%)",
        stats.large_inversions,
        (stats.large_inversions as f64 / stats.inversions as f64) * 100.0
    );
    
    // Pattern analysis
    println!("\n🔄 Pattern Analysis:");
    println!("   Isolated inversions: {}", stats.isolated_inversions);
    println!("   Consecutive inversions: {}", stats.consecutive_inversions);
    println!("   Max consecutive: {}", stats.max_consecutive);
    
    // Exchange breakdown
    if !stats.exchange_inversions.is_empty() {
        println!("\n🏢 By Exchange:");
        for (exchange, inv_count) in &stats.exchange_inversions {
            let total = stats.exchange_packets.get(exchange).unwrap_or(&1);
            let ex_rate = (*inv_count as f64 / *total as f64) * 100.0;
            println!("   {} - {} inversions ({:.3}% of {} packets)",
                exchange.bright_yellow(),
                inv_count.to_string().red(),
                ex_rate,
                total
            );
        }
    }
    
    // Diagnosis
    println!("\n💡 LIKELY CAUSE ANALYSIS:");
    
    // Check if it's mostly tiny inversions
    if stats.tiny_inversions > stats.inversions * 7 / 10 {
        println!("   🌐 Mostly tiny inversions (<10ms) → {} reordering",
            "NETWORK".bright_blue()
        );
        println!("      UDP packets arriving slightly out of order");
        println!("      This is normal for high-frequency UDP multicast");
    }
    // Check if large inversions dominate
    else if stats.large_inversions > stats.inversions / 3 {
        println!("   📡 Many large inversions (>1s) → {} issues",
            "SOURCE".bright_red()
        );
        println!("      Exchange sending wrong timestamps or");
        println!("      Timestamp format issues (mixing seconds/milliseconds)");
    }
    // Check for consecutive patterns
    else if stats.consecutive_inversions > stats.isolated_inversions {
        println!("   🔄 Consecutive inversion patterns → {} buffering",
            "SOURCE".bright_yellow()
        );
        println!("      Exchange or feeder batching/buffering data");
        println!("      Releasing batches out of order");
    }
    // Check if specific exchange
    else if stats.exchange_inversions.len() == 1 {
        let exchange = stats.exchange_inversions.keys().next().unwrap();
        println!("   🏢 Only {} affected → {}-specific issue",
            exchange.bright_yellow(),
            "EXCHANGE".bright_red()
        );
        println!("      Problem isolated to one exchange");
    }
    else {
        println!("   🔀 Mixed patterns → {} causes",
            "MULTIPLE".bright_yellow()
        );
        println!("      Both network reordering and source issues");
    }
    
    // Recommendations
    println!("\n📝 RECOMMENDATIONS:");
    if stats.tiny_inversions > stats.inversions / 2 {
        println!("   • Small buffering window (50-100ms) would fix most issues");
    }
    if stats.large_inversions > 0 {
        println!("   • Check timestamp format handling in exchange adapters");
        println!("   • Run: cargo run --bin timestamp_format_analyzer");
    }
    if stats.consecutive_inversions > stats.isolated_inversions {
        println!("   • Review exchange reconnection and recovery logic");
    }
    
    println!("\n{}", "═".repeat(80).bright_cyan());
}