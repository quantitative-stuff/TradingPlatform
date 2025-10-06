use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
struct TimestampFormat {
    digit_count: usize,
    samples: Vec<i64>,
    likely_format: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🕐 TIMESTAMP FORMAT ANALYZER".bright_cyan().bold());
    println!("Detecting mixed timestamp formats from different exchanges...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Analyzing timestamp formats per exchange...\n");
    
    let mut buf = vec![0u8; 65536];
    let mut exchange_formats: HashMap<String, TimestampFormat> = HashMap::new();
    let mut format_issues: Vec<String> = Vec::new();
    let mut packet_count = 0u64;
    
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
                            packet_count += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown").to_string();
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            if timestamp > 0 {
                                let digit_count = timestamp.to_string().len();
                                
                                // Get or create format entry
                                let format = exchange_formats.entry(exchange.clone()).or_insert(TimestampFormat {
                                    digit_count,
                                    samples: Vec::new(),
                                    likely_format: String::new(),
                                });
                                
                                // Detect format change
                                if format.digit_count != digit_count {
                                    let issue = format!("⚠️ {} format inconsistency! Had {} digits, now {} digits ({})", 
                                        exchange.bright_yellow(), 
                                        format.digit_count, 
                                        digit_count,
                                        symbol
                                    );
                                    println!("{}", issue.bright_red());
                                    format_issues.push(issue);
                                    
                                    // Show the problematic timestamps
                                    println!("   Previous sample: {} ({} digits)", 
                                        format.samples.last().unwrap_or(&0), 
                                        format.digit_count
                                    );
                                    println!("   Current sample:  {} ({} digits)", 
                                        timestamp, 
                                        digit_count
                                    );
                                    
                                    // Try to interpret both ways
                                    analyze_timestamp(timestamp, &exchange, symbol);
                                }
                                
                                // Update format tracking
                                format.digit_count = digit_count;
                                format.samples.push(timestamp);
                                if format.samples.len() > 10 {
                                    format.samples.remove(0);
                                }
                                
                                // Determine likely format
                                format.likely_format = match digit_count {
                                    10 => "seconds".to_string(),
                                    13 => "milliseconds".to_string(),
                                    16 => "microseconds".to_string(),
                                    19 => "nanoseconds".to_string(),
                                    _ => format!("{} digits (unknown)", digit_count),
                                };
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Error: {}", e);
            }
            Err(_) => {
                // Timeout - print summary
                if packet_count > 0 {
                    print_summary(&exchange_formats, &format_issues);
                }
            }
        }
        
        // Print detailed analysis every 100 packets
        if packet_count % 100 == 0 && packet_count > 0 {
            print_summary(&exchange_formats, &format_issues);
        }
    }
}

fn analyze_timestamp(ts: i64, exchange: &str, symbol: &str) {
    println!("\n📊 Analyzing timestamp {} from {}:{}", ts, exchange, symbol);
    
    let digit_count = ts.to_string().len();
    let now = Utc::now();
    
    // Try different interpretations
    println!("   Digit count: {}", digit_count);
    
    match digit_count {
        10 => {
            // Likely seconds
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                println!("   As seconds: {} ({})", 
                    dt.format("%Y-%m-%d %H:%M:%S UTC"),
                    if dt > now { "FUTURE!".red() } else { "past".green() }
                );
                
                // Convert to milliseconds
                let as_millis = ts * 1000;
                println!("   Converted to ms: {} (standard format)", as_millis.to_string().bright_green());
            }
        }
        13 => {
            // Likely milliseconds (standard)
            let seconds = ts / 1000;
            if let Some(dt) = DateTime::from_timestamp(seconds, 0) {
                println!("   As milliseconds: {} ({})",
                    dt.format("%Y-%m-%d %H:%M:%S UTC"),
                    if dt > now { "FUTURE!".red() } else { "ok".green() }
                );
            }
        }
        _ => {
            // Try as seconds anyway
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                println!("   Interpreted as seconds: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"));
            }
            // Try as milliseconds
            let as_seconds = ts / 1000;
            if let Some(dt) = DateTime::from_timestamp(as_seconds, 0) {
                println!("   Interpreted as milliseconds: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"));
            }
        }
    }
    
    // Check if it's a sequence number instead
    if ts < 1_000_000_000 {
        println!("   ⚠️ Might be a sequence number, not a timestamp!");
    }
}

fn print_summary(formats: &HashMap<String, TimestampFormat>, issues: &Vec<String>) {
    print!("\x1B[2J\x1B[1;1H");
    println!("{}", "📊 TIMESTAMP FORMAT ANALYSIS".bright_cyan().bold());
    println!("{}", "═".repeat(70).bright_cyan());
    
    println!("\n🏢 Exchange Timestamp Formats:");
    println!("{}", "─".repeat(70).bright_blue());
    
    for (exchange, format) in formats {
        let status = if format.likely_format == "milliseconds" {
            "✅".green()
        } else {
            "⚠️".yellow()
        };
        
        println!("{} {} - {} ({} digits)",
            status,
            exchange.bright_white(),
            format.likely_format.bright_yellow(),
            format.digit_count
        );
        
        if !format.samples.is_empty() {
            let latest = format.samples.last().unwrap();
            println!("     Latest: {}", latest);
            
            // Show human-readable time
            let (seconds, label) = match format.digit_count {
                10 => (*latest, "direct"),
                13 => (*latest / 1000, "from ms"),
                16 => (*latest / 1_000_000, "from μs"),
                _ => (*latest, "unknown"),
            };
            
            if let Some(dt) = DateTime::from_timestamp(seconds, 0) {
                println!("     Time: {} ({})", 
                    dt.format("%Y-%m-%d %H:%M:%S UTC"),
                    label
                );
            }
        }
    }
    
    if !issues.is_empty() {
        println!("\n⚠️ FORMAT INCONSISTENCIES DETECTED:");
        println!("{}", "─".repeat(70).bright_red());
        for issue in issues.iter().rev().take(5) {
            println!("{}", issue);
        }
    }
    
    // Recommendations
    println!("\n💡 RECOMMENDATIONS:");
    let non_standard: Vec<_> = formats.iter()
        .filter(|(_, f)| f.likely_format != "milliseconds")
        .collect();
    
    if !non_standard.is_empty() {
        println!("   🔴 Non-standard timestamp formats detected!");
        for (exchange, format) in non_standard {
            println!("      {} uses {} - needs conversion to milliseconds",
                exchange.bright_yellow(),
                format.likely_format.bright_red()
            );
        }
        println!("\n   FIX: Normalize all timestamps to milliseconds (13 digits)");
        println!("   - For 10-digit (seconds): multiply by 1000");
        println!("   - For 16-digit (microseconds): divide by 1000");
    } else if issues.is_empty() {
        println!("   ✅ All exchanges using standard millisecond timestamps");
    }
    
    println!("\n{}", "═".repeat(70).bright_cyan());
}