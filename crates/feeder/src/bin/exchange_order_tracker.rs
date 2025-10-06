use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Default)]
struct ExchangeMetrics {
    packets_received: u64,
    inversions: u64,
    last_timestamp: i64,
    max_gap: i64,
    min_gap: i64,
    avg_gap: i64,
    gap_count: u64,
    symbols: HashMap<String, SymbolMetrics>,
}

#[derive(Debug, Default)]
struct SymbolMetrics {
    packets: u64,
    inversions: u64,
    last_timestamp: i64,
    inversion_sizes: Vec<i64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🏢 EXCHANGE-SPECIFIC ORDER TRACKER".bright_cyan().bold());
    println!("Tracking packet ordering by exchange to identify problematic sources...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001");
    println!("Tracking exchange-specific ordering...\n");
    
    let mut buf = vec![0u8; 65536];
    let mut exchanges: HashMap<String, ExchangeMetrics> = HashMap::new();
    let mut total_packets = 0u64;
    let mut total_inversions = 0u64;
    
    let start_time = Instant::now();
    let mut last_display = Instant::now();
    
    // Known exchanges
    let known_exchanges = vec![
        "Binance", "Bybit", "Upbit", "Coinbase", "OKX", "Deribit", "Bithumb"
    ];
    
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
                            total_packets += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown").to_string();
                            let symbol = json["symbol"].as_str().unwrap_or("unknown").to_string();
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            let metrics = exchanges.entry(exchange.clone()).or_insert(ExchangeMetrics::default());
                            metrics.packets_received += 1;
                            
                            // Check symbol-level ordering
                            let symbol_metrics = metrics.symbols.entry(symbol.clone()).or_insert(SymbolMetrics::default());
                            symbol_metrics.packets += 1;
                            
                            if symbol_metrics.last_timestamp > 0 {
                                let gap = timestamp - symbol_metrics.last_timestamp;
                                
                                if gap < 0 {
                                    // Inversion detected
                                    metrics.inversions += 1;
                                    symbol_metrics.inversions += 1;
                                    symbol_metrics.inversion_sizes.push(gap.abs());
                                    total_inversions += 1;
                                    
                                    // Real-time alert for new exchange inversions
                                    if metrics.inversions == 1 {
                                        println!("\n{} First inversion detected for {}!", 
                                            "🚨".bright_red(), 
                                            exchange.bright_yellow()
                                        );
                                    }
                                } else {
                                    // Normal ordering - track gaps
                                    metrics.gap_count += 1;
                                    metrics.avg_gap = ((metrics.avg_gap * (metrics.gap_count - 1) as i64) + gap) / metrics.gap_count as i64;
                                    
                                    if metrics.max_gap == 0 || gap > metrics.max_gap {
                                        metrics.max_gap = gap;
                                    }
                                    if metrics.min_gap == 0 || gap < metrics.min_gap {
                                        metrics.min_gap = gap;
                                    }
                                }
                            }
                            
                            symbol_metrics.last_timestamp = timestamp;
                            metrics.last_timestamp = timestamp;
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
        
        // Update display every 2 seconds
        if last_display.elapsed().as_secs() >= 2 {
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", "🏢 EXCHANGE-SPECIFIC ORDER TRACKER".bright_cyan().bold());
            println!("{}", "═".repeat(80).bright_cyan());
            
            println!("\n⏱️  Runtime: {:.1}s", start_time.elapsed().as_secs_f64());
            println!("📦 Total packets: {}", total_packets.to_string().bright_yellow());
            println!("⚠️  Total inversions: {}", total_inversions.to_string().bright_red());
            
            if total_packets > 0 {
                let overall_rate = (total_inversions as f64 / total_packets as f64) * 100.0;
                println!("📊 Overall inversion rate: {:.4}%", overall_rate);
            }
            
            println!("\n{}", "Exchange Analysis:".bright_white());
            println!("{}", "─".repeat(80).bright_blue());
            
            // Sort exchanges by packet count
            let mut sorted_exchanges: Vec<_> = exchanges.iter().collect();
            sorted_exchanges.sort_by(|a, b| b.1.packets_received.cmp(&a.1.packets_received));
            
            for (exchange, metrics) in sorted_exchanges {
                let status = if metrics.inversions > 0 { "❌" } else { "✅" };
                let rate = if metrics.packets_received > 0 {
                    (metrics.inversions as f64 / metrics.packets_received as f64) * 100.0
                } else {
                    0.0
                };
                
                println!("\n{} {} ({})", 
                    status,
                    exchange.bright_yellow(),
                    if known_exchanges.contains(&exchange.as_str()) { 
                        "known".green() 
                    } else { 
                        "unknown".red() 
                    }
                );
                
                println!("   Packets: {}", metrics.packets_received.to_string().cyan());
                
                if metrics.inversions > 0 {
                    println!("   Inversions: {} ({:.4}%)", 
                        metrics.inversions.to_string().bright_red(),
                        rate
                    );
                    
                    // Show top problematic symbols
                    let mut problem_symbols: Vec<_> = metrics.symbols.iter()
                        .filter(|(_, s)| s.inversions > 0)
                        .collect();
                    problem_symbols.sort_by(|a, b| b.1.inversions.cmp(&a.1.inversions));
                    
                    if !problem_symbols.is_empty() {
                        println!("   Problem symbols:");
                        for (symbol, sym_metrics) in problem_symbols.iter().take(3) {
                            let sym_rate = (sym_metrics.inversions as f64 / sym_metrics.packets as f64) * 100.0;
                            println!("      {} - {} inversions ({:.2}%)",
                                symbol.yellow(),
                                sym_metrics.inversions,
                                sym_rate
                            );
                            
                            if !sym_metrics.inversion_sizes.is_empty() {
                                let avg_size = sym_metrics.inversion_sizes.iter().sum::<i64>() / sym_metrics.inversion_sizes.len() as i64;
                                let max_size = *sym_metrics.inversion_sizes.iter().max().unwrap();
                                println!("         Avg jump: {}ms, Max: {}ms", avg_size, max_size);
                            }
                        }
                    }
                } else {
                    println!("   Status: {} All packets in order", "✓".green());
                }
                
                if metrics.gap_count > 0 {
                    println!("   Timing: min gap {}ms, avg {}ms, max {}ms",
                        metrics.min_gap,
                        metrics.avg_gap,
                        metrics.max_gap
                    );
                }
                
                println!("   Active symbols: {}", metrics.symbols.len());
            }
            
            // Summary insights
            println!("\n{}", "─".repeat(80).bright_blue());
            println!("{} INSIGHTS:", "💡".bright_white());
            
            let exchanges_with_inversions = exchanges.values()
                .filter(|m| m.inversions > 0)
                .count();
            
            if exchanges_with_inversions == 0 {
                println!("   ✅ All exchanges delivering packets in order!");
            } else if exchanges_with_inversions == exchanges.len() {
                println!("   ⚠️  All exchanges showing inversions - likely network issue");
            } else {
                println!("   📍 {} of {} exchanges have inversions", 
                    exchanges_with_inversions, 
                    exchanges.len()
                );
                
                // Find worst performer
                if let Some((worst_exchange, worst_metrics)) = exchanges.iter()
                    .max_by_key(|(_, m)| m.inversions) {
                    if worst_metrics.inversions > 0 {
                        let worst_rate = (worst_metrics.inversions as f64 / worst_metrics.packets_received as f64) * 100.0;
                        println!("   🔴 Worst: {} with {:.3}% inversion rate",
                            worst_exchange.bright_red(),
                            worst_rate
                        );
                    }
                }
            }
            
            last_display = Instant::now();
        }
    }
}