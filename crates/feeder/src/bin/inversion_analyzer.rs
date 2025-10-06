use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔍 INVERSION ANALYZER".bright_cyan().bold());
    println!("Tracking packet order to find out-of-order patterns...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001");
    println!("Monitoring for inversions...\n");
    println!("{}", "─".repeat(70).bright_blue());
    
    let mut buf = vec![0u8; 65536];
    let mut last_timestamps: HashMap<String, i64> = HashMap::new();
    let mut packet_count: HashMap<String, u64> = HashMap::new();
    let mut inversion_patterns: HashMap<String, Vec<(i64, i64, i64)>> = HashMap::new(); // (prev, curr, diff)
    let mut total_packets = 0u64;
    let start_time = Instant::now();
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, _addr))) => {
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            total_packets += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown");
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            let key = format!("{}:{}", exchange, symbol);
                            *packet_count.entry(key.clone()).or_insert(0) += 1;
                            
                            // Check for inversion
                            if let Some(&last_ts) = last_timestamps.get(&key) {
                                if timestamp < last_ts {
                                    let diff = last_ts - timestamp;
                                    
                                    // Detailed analysis of the inversion
                                    println!("\n{} INVERSION DETECTED!", "⚠️".bright_red());
                                    println!("   Symbol: {}", key.bright_yellow());
                                    println!("   Previous timestamp: {} ({})", 
                                        last_ts, 
                                        format_timestamp(last_ts).bright_white()
                                    );
                                    println!("   Current timestamp:  {} ({})", 
                                        timestamp, 
                                        format_timestamp(timestamp).bright_white()
                                    );
                                    println!("   Went backwards by: {} ms", diff.to_string().bright_red());
                                    
                                    // Analyze the pattern
                                    let patterns = inversion_patterns.entry(key.clone()).or_insert(Vec::new());
                                    patterns.push((last_ts, timestamp, diff));
                                    
                                    // Check if it's a huge jump (likely format issue)
                                    if diff > 1_000_000_000 {  // More than 1 billion ms (11.5 days)
                                        println!("   {} HUGE JUMP - Likely timestamp format issue!", "🚨".bright_red());
                                        
                                        // Try different interpretations
                                        println!("\n   Possible interpretations:");
                                        
                                        // If current is 10 digits, might be seconds
                                        let curr_digits = timestamp.to_string().len();
                                        let prev_digits = last_ts.to_string().len();
                                        
                                        if curr_digits == 10 && prev_digits == 13 {
                                            let curr_as_seconds_to_ms = timestamp * 1000;
                                            println!("   If {} is seconds → {} ms", 
                                                timestamp, curr_as_seconds_to_ms);
                                            println!("   That would be: {}", 
                                                format_timestamp(curr_as_seconds_to_ms).yellow());
                                            
                                            if curr_as_seconds_to_ms > last_ts {
                                                println!("   ✅ Would be in correct order if converted!");
                                            }
                                        }
                                        
                                        if prev_digits == 10 && curr_digits == 13 {
                                            let prev_as_seconds_to_ms = last_ts * 1000;
                                            println!("   If {} was seconds → {} ms", 
                                                last_ts, prev_as_seconds_to_ms);
                                            println!("   That would be: {}", 
                                                format_timestamp(prev_as_seconds_to_ms).yellow());
                                        }
                                    } else if diff < 1000 {  // Less than 1 second
                                        println!("   💡 Small inversion (<1s) - likely network reordering");
                                    } else if diff < 60000 {  // Less than 1 minute
                                        println!("   💡 Medium inversion (<1min) - possible batching issue");
                                    }
                                    
                                    // Show packet counts
                                    let count = packet_count.get(&key).unwrap_or(&0);
                                    let inversions = patterns.len();
                                    let rate = (inversions as f64 / *count as f64) * 100.0;
                                    println!("   Stats: {} inversions out of {} packets ({:.2}%)",
                                        inversions, count, rate);
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
                // Timeout after 30 seconds - print summary
                break;
            }
        }
        
        // Print summary every 100 packets
        if total_packets % 100 == 0 && total_packets > 0 {
            println!("\n{} Processed {} packets in {:.1}s", 
                "📊".bright_cyan(),
                total_packets, 
                start_time.elapsed().as_secs_f64()
            );
        }
    }
    
    // Final summary
    println!("\n{}", "═".repeat(70).bright_cyan());
    println!("{} FINAL ANALYSIS", "📊".bright_white());
    println!("{}", "─".repeat(70).bright_blue());
    
    println!("\nTotal packets: {}", total_packets);
    println!("Runtime: {:.1}s", start_time.elapsed().as_secs_f64());
    
    if !inversion_patterns.is_empty() {
        println!("\n{} Symbols with inversions:", "⚠️".yellow());
        
        for (symbol, patterns) in &inversion_patterns {
            let count = packet_count.get(symbol).unwrap_or(&0);
            let rate = (patterns.len() as f64 / *count as f64) * 100.0;
            
            println!("\n  {} ({} inversions, {:.2}% rate)", 
                symbol.bright_yellow(), 
                patterns.len(), 
                rate
            );
            
            // Analyze the pattern
            let max_diff = patterns.iter().map(|(_, _, d)| d).max().unwrap_or(&0);
            let min_diff = patterns.iter().map(|(_, _, d)| d).min().unwrap_or(&0);
            let avg_diff = patterns.iter().map(|(_, _, d)| d).sum::<i64>() / patterns.len() as i64;
            
            println!("    Backward jumps: min={}ms, avg={}ms, max={}ms", 
                min_diff, avg_diff, max_diff);
            
            if *max_diff > 1_000_000_000 {
                println!("    {} Likely timestamp format issue (jumps > 1 billion ms)", "🚨".red());
            } else if *max_diff < 1000 {
                println!("    {} Likely network reordering (jumps < 1 second)", "💡".green());
            }
        }
    } else {
        println!("\n{} No inversions detected!", "✅".bright_green());
    }
    
    Ok(())
}

fn format_timestamp(ts: i64) -> String {
    // Assume milliseconds
    let seconds = ts / 1000;
    let millis = (ts % 1000) as u32;
    
    if let Some(dt) = chrono::DateTime::from_timestamp(seconds, millis * 1_000_000) {
        dt.format("%H:%M:%S.%3f").to_string()
    } else {
        format!("Invalid({})", ts)
    }
}