use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::fs::File;
use std::io::Write;
use chrono::{DateTime, Local};

#[derive(Debug, Clone)]
struct Statistics {
    count: u64,
    sum: f64,
    sum_squared: f64,
    min: f64,
    max: f64,
}

impl Statistics {
    fn new() -> Self {
        Statistics {
            count: 0,
            sum: 0.0,
            sum_squared: 0.0,
            min: f64::MAX,
            max: f64::MIN,
        }
    }
    
    fn update(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        self.sum_squared += value * value;
        if value < self.min { self.min = value; }
        if value > self.max { self.max = value; }
    }
    
    fn mean(&self) -> f64 {
        if self.count > 0 {
            self.sum / self.count as f64
        } else {
            0.0
        }
    }
    
    fn std_dev(&self) -> f64 {
        if self.count > 1 {
            let variance = (self.sum_squared - (self.sum * self.sum / self.count as f64)) / (self.count - 1) as f64;
            variance.max(0.0).sqrt()
        } else {
            0.0
        }
    }
}

#[derive(Debug)]
struct OrderingReport {
    start_time: Instant,
    total_packets: u64,
    total_inversions: u64,
    
    // Statistics by exchange
    exchange_stats: HashMap<String, ExchangeStats>,
    
    // Global statistics
    latency_stats: Statistics,
    inversion_size_stats: Statistics,
    packet_interval_stats: Statistics,
    
    // Time-based analysis
    inversions_per_minute: Vec<u64>,
    packets_per_minute: Vec<u64>,
}

#[derive(Debug)]
struct ExchangeStats {
    packets: u64,
    inversions: u64,
    symbols: HashMap<String, SymbolStats>,
    latency_stats: Statistics,
    interval_stats: Statistics,
}

#[derive(Debug)]
struct SymbolStats {
    packets: u64,
    inversions: u64,
    last_timestamp: i64,
    inversion_sizes: Vec<i64>,
}

impl OrderingReport {
    fn new() -> Self {
        OrderingReport {
            start_time: Instant::now(),
            total_packets: 0,
            total_inversions: 0,
            exchange_stats: HashMap::new(),
            latency_stats: Statistics::new(),
            inversion_size_stats: Statistics::new(),
            packet_interval_stats: Statistics::new(),
            inversions_per_minute: Vec::new(),
            packets_per_minute: Vec::new(),
        }
    }
    
    fn generate_report(&self) -> String {
        let mut report = String::new();
        
        report.push_str(&format!("# PACKET ORDERING STATISTICAL REPORT\n"));
        report.push_str(&format!("Generated: {}\n", Local::now().format("%Y-%m-%d %H:%M:%S")));
        report.push_str(&format!("Runtime: {:.1} seconds\n\n", self.start_time.elapsed().as_secs_f64()));
        
        report.push_str("## SUMMARY\n");
        report.push_str(&format!("- Total Packets: {}\n", self.total_packets));
        report.push_str(&format!("- Total Inversions: {}\n", self.total_inversions));
        if self.total_packets > 0 {
            let rate = (self.total_inversions as f64 / self.total_packets as f64) * 100.0;
            report.push_str(&format!("- Overall Inversion Rate: {:.4}%\n", rate));
        }
        
        report.push_str("\n## LATENCY STATISTICS\n");
        report.push_str(&format!("- Mean: {:.2}ms\n", self.latency_stats.mean()));
        report.push_str(&format!("- Std Dev: {:.2}ms\n", self.latency_stats.std_dev()));
        report.push_str(&format!("- Min: {:.2}ms\n", self.latency_stats.min));
        report.push_str(&format!("- Max: {:.2}ms\n", self.latency_stats.max));
        
        if self.total_inversions > 0 {
            report.push_str("\n## INVERSION SIZE STATISTICS\n");
            report.push_str(&format!("- Mean: {:.2}ms\n", self.inversion_size_stats.mean()));
            report.push_str(&format!("- Std Dev: {:.2}ms\n", self.inversion_size_stats.std_dev()));
            report.push_str(&format!("- Min: {:.2}ms\n", self.inversion_size_stats.min));
            report.push_str(&format!("- Max: {:.2}ms\n", self.inversion_size_stats.max));
        }
        
        report.push_str("\n## EXCHANGE BREAKDOWN\n");
        let mut sorted_exchanges: Vec<_> = self.exchange_stats.iter().collect();
        sorted_exchanges.sort_by(|a, b| b.1.inversions.cmp(&a.1.inversions));
        
        for (exchange, stats) in sorted_exchanges {
            let rate = if stats.packets > 0 {
                (stats.inversions as f64 / stats.packets as f64) * 100.0
            } else {
                0.0
            };
            
            report.push_str(&format!("\n### {}\n", exchange));
            report.push_str(&format!("- Packets: {}\n", stats.packets));
            report.push_str(&format!("- Inversions: {} ({:.4}%)\n", stats.inversions, rate));
            report.push_str(&format!("- Active Symbols: {}\n", stats.symbols.len()));
            report.push_str(&format!("- Avg Latency: {:.2}ms\n", stats.latency_stats.mean()));
            
            // Top problematic symbols
            if stats.inversions > 0 {
                let mut problem_symbols: Vec<_> = stats.symbols.iter()
                    .filter(|(_, s)| s.inversions > 0)
                    .collect();
                problem_symbols.sort_by(|a, b| b.1.inversions.cmp(&a.1.inversions));
                
                if !problem_symbols.is_empty() {
                    report.push_str("- Top Problem Symbols:\n");
                    for (symbol, sym_stats) in problem_symbols.iter().take(3) {
                        let sym_rate = (sym_stats.inversions as f64 / sym_stats.packets as f64) * 100.0;
                        report.push_str(&format!("  - {}: {} inversions ({:.2}%)\n", 
                            symbol, sym_stats.inversions, sym_rate));
                    }
                }
            }
        }
        
        report
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "📊 STATISTICAL ORDER REPORT GENERATOR".bright_cyan().bold());
    println!("Collecting comprehensive statistics on packet ordering...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001");
    println!("Collecting data for statistical analysis...\n");
    println!("Report will be generated every 60 seconds\n");
    
    let mut buf = vec![0u8; 65536];
    let mut report = OrderingReport::new();
    let mut last_timestamps: HashMap<String, i64> = HashMap::new();
    let mut last_report_time = Instant::now();
    let mut minute_counter = 0;
    let mut current_minute_packets = 0u64;
    let mut current_minute_inversions = 0u64;
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, _addr))) => {
                let receive_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;
                
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            report.total_packets += 1;
                            current_minute_packets += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown").to_string();
                            let symbol = json["symbol"].as_str().unwrap_or("unknown").to_string();
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            let key = format!("{}:{}", exchange, symbol);
                            
                            // Calculate latency
                            let latency = (receive_time - timestamp) as f64;
                            report.latency_stats.update(latency);
                            
                            // Get or create exchange stats
                            let exchange_stats = report.exchange_stats
                                .entry(exchange.clone())
                                .or_insert(ExchangeStats {
                                    packets: 0,
                                    inversions: 0,
                                    symbols: HashMap::new(),
                                    latency_stats: Statistics::new(),
                                    interval_stats: Statistics::new(),
                                });
                            
                            exchange_stats.packets += 1;
                            exchange_stats.latency_stats.update(latency);
                            
                            // Get or create symbol stats
                            let symbol_stats = exchange_stats.symbols
                                .entry(symbol.clone())
                                .or_insert(SymbolStats {
                                    packets: 0,
                                    inversions: 0,
                                    last_timestamp: 0,
                                    inversion_sizes: Vec::new(),
                                });
                            
                            symbol_stats.packets += 1;
                            
                            // Check for inversion
                            if let Some(&last_ts) = last_timestamps.get(&key) {
                                let interval = (timestamp - last_ts) as f64;
                                
                                if timestamp < last_ts {
                                    // Inversion detected
                                    let inversion_size = (last_ts - timestamp) as f64;
                                    
                                    report.total_inversions += 1;
                                    current_minute_inversions += 1;
                                    report.inversion_size_stats.update(inversion_size);
                                    
                                    exchange_stats.inversions += 1;
                                    symbol_stats.inversions += 1;
                                    symbol_stats.inversion_sizes.push(inversion_size as i64);
                                } else {
                                    // Normal ordering - track interval
                                    report.packet_interval_stats.update(interval.abs());
                                    exchange_stats.interval_stats.update(interval.abs());
                                }
                            }
                            
                            symbol_stats.last_timestamp = timestamp;
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
        
        // Update minute statistics
        if report.start_time.elapsed().as_secs() / 60 > minute_counter {
            minute_counter += 1;
            report.packets_per_minute.push(current_minute_packets);
            report.inversions_per_minute.push(current_minute_inversions);
            current_minute_packets = 0;
            current_minute_inversions = 0;
        }
        
        // Generate report every 60 seconds
        if last_report_time.elapsed().as_secs() >= 60 {
            let report_content = report.generate_report();
            
            // Save to file
            let filename = format!("order_report_{}.txt", 
                Local::now().format("%Y%m%d_%H%M%S"));
            
            match File::create(&filename) {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(report_content.as_bytes()) {
                        eprintln!("Failed to write report: {}", e);
                    } else {
                        println!("\n{} Report saved to: {}", "✅".green(), filename.bright_yellow());
                    }
                }
                Err(e) => {
                    eprintln!("Failed to create report file: {}", e);
                }
            }
            
            // Display summary to console
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", "📊 STATISTICAL SUMMARY".bright_cyan().bold());
            println!("{}", "═".repeat(80).bright_cyan());
            
            println!("\n⏱️  Runtime: {:.1}s", report.start_time.elapsed().as_secs_f64());
            println!("📦 Total Packets: {}", report.total_packets.to_string().bright_yellow());
            println!("⚠️  Total Inversions: {}", report.total_inversions.to_string().bright_red());
            
            if report.total_packets > 0 {
                let rate = (report.total_inversions as f64 / report.total_packets as f64) * 100.0;
                println!("📈 Inversion Rate: {:.4}%", rate);
            }
            
            println!("\n📊 Network Latency:");
            println!("   Mean: {:.2}ms ± {:.2}ms", 
                report.latency_stats.mean(), 
                report.latency_stats.std_dev()
            );
            println!("   Range: {:.2}ms - {:.2}ms", 
                report.latency_stats.min,
                report.latency_stats.max
            );
            
            if report.total_inversions > 0 {
                println!("\n📏 Inversion Sizes:");
                println!("   Mean: {:.2}ms ± {:.2}ms",
                    report.inversion_size_stats.mean(),
                    report.inversion_size_stats.std_dev()
                );
                println!("   Range: {:.2}ms - {:.2}ms",
                    report.inversion_size_stats.min,
                    report.inversion_size_stats.max
                );
            }
            
            println!("\n🏢 Exchange Summary:");
            let mut sorted: Vec<_> = report.exchange_stats.iter().collect();
            sorted.sort_by(|a, b| b.1.packets.cmp(&a.1.packets));
            
            for (exchange, stats) in sorted.iter().take(5) {
                let rate = if stats.packets > 0 {
                    (stats.inversions as f64 / stats.packets as f64) * 100.0
                } else {
                    0.0
                };
                
                let status = if stats.inversions == 0 { "✅" } else { "⚠️" };
                println!("   {} {} - {} packets, {} inversions ({:.3}%)",
                    status,
                    exchange.bright_yellow(),
                    stats.packets,
                    stats.inversions,
                    rate
                );
            }
            
            println!("\n📝 Report saved to: {}", filename.bright_green());
            println!("{}", "═".repeat(80).bright_cyan());
            
            last_report_time = Instant::now();
        }
    }
}