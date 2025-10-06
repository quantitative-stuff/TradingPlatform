use tokio::net::UdpSocket;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Instant, Duration};
use colored::*;
use serde_json;
use chrono::Local;
use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
struct SymbolOrderStats {
    last_timestamp: i64,
    last_sequence: u64,
    inversions: u64,
    total_packets: u64,
    max_inversion_ms: i64,
    inversion_distances: Vec<i64>,
    last_update: Instant,
}

impl Default for SymbolOrderStats {
    fn default() -> Self {
        Self {
            last_timestamp: 0,
            last_sequence: 0,
            inversions: 0,
            total_packets: 0,
            max_inversion_ms: 0,
            inversion_distances: Vec::new(),
            last_update: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
struct ExchangeOrderStats {
    symbols: HashMap<String, SymbolOrderStats>,
    total_packets: u64,
    total_inversions: u64,
    packet_gaps: u64,  // Detected sequence gaps
    max_gap_size: u64,
    start_time: Instant,
}

impl Default for ExchangeOrderStats {
    fn default() -> Self {
        Self {
            symbols: HashMap::new(),
            total_packets: 0,
            total_inversions: 0,
            packet_gaps: 0,
            max_gap_size: 0,
            start_time: Instant::now(),
        }
    }
}

#[derive(Clone)]
struct OrderValidator {
    exchanges: Arc<Mutex<HashMap<String, ExchangeOrderStats>>>,
    last_display: Arc<Mutex<Instant>>,
    display_interval: Duration,
}

impl OrderValidator {
    fn new() -> Self {
        Self {
            exchanges: Arc::new(Mutex::new(HashMap::new())),
            last_display: Arc::new(Mutex::new(Instant::now())),
            display_interval: Duration::from_secs(5),
        }
    }

    async fn process_packet(&self, data: &str) {
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() < 2 {
            return;
        }

        match parts[0] {
            "TRADE" => {
                if let Ok(trade) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                    self.process_trade(&trade).await;
                }
            }
            "ORDERBOOK" => {
                if let Ok(orderbook) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                    self.process_orderbook(&orderbook).await;
                }
            }
            _ => {}
        }

        // Check if it's time to display stats
        let mut last_display = self.last_display.lock().await;
        if last_display.elapsed() >= self.display_interval {
            self.display_stats().await;
            *last_display = Instant::now();
        }
    }

    async fn process_trade(&self, trade: &serde_json::Value) {
        let exchange = trade["exchange"].as_str().unwrap_or("unknown");
        let symbol = trade["symbol"].as_str().unwrap_or("unknown");
        let timestamp = trade["timestamp"].as_i64().unwrap_or(0);
        let sequence = trade["sequence"].as_u64().unwrap_or(0);

        let mut exchanges = self.exchanges.lock().await;
        let exchange_stats = exchanges.entry(exchange.to_string()).or_default();
        let symbol_stats = exchange_stats.symbols.entry(symbol.to_string()).or_default();

        // Update packet count
        symbol_stats.total_packets += 1;
        exchange_stats.total_packets += 1;

        // Check for timestamp inversion
        if timestamp < symbol_stats.last_timestamp && symbol_stats.last_timestamp > 0 {
            symbol_stats.inversions += 1;
            exchange_stats.total_inversions += 1;
            
            let inversion_ms = symbol_stats.last_timestamp - timestamp;
            symbol_stats.inversion_distances.push(inversion_ms);
            
            if inversion_ms > symbol_stats.max_inversion_ms {
                symbol_stats.max_inversion_ms = inversion_ms;
            }
        }

        // Check for sequence gaps (if sequence numbers are provided)
        if sequence > 0 && symbol_stats.last_sequence > 0 {
            let expected_seq = symbol_stats.last_sequence + 1;
            if sequence != expected_seq {
                exchange_stats.packet_gaps += 1;
                let gap_size = if sequence > expected_seq {
                    sequence - expected_seq
                } else {
                    0  // Out of order, not a gap
                };
                
                if gap_size > exchange_stats.max_gap_size {
                    exchange_stats.max_gap_size = gap_size;
                }
            }
        }

        symbol_stats.last_timestamp = timestamp;
        symbol_stats.last_sequence = sequence;
        symbol_stats.last_update = Instant::now();
    }

    async fn process_orderbook(&self, orderbook: &serde_json::Value) {
        let exchange = orderbook["exchange"].as_str().unwrap_or("unknown");
        let symbol = orderbook["symbol"].as_str().unwrap_or("unknown");
        let timestamp = orderbook["timestamp"].as_i64().unwrap_or(0);
        let sequence = orderbook["sequence"].as_u64().unwrap_or(0);

        let mut exchanges = self.exchanges.lock().await;
        let exchange_stats = exchanges.entry(exchange.to_string()).or_default();
        let symbol_stats = exchange_stats.symbols.entry(symbol.to_string()).or_default();

        // Update packet count
        symbol_stats.total_packets += 1;
        exchange_stats.total_packets += 1;

        // Check for timestamp inversion
        if timestamp < symbol_stats.last_timestamp && symbol_stats.last_timestamp > 0 {
            symbol_stats.inversions += 1;
            exchange_stats.total_inversions += 1;
            
            let inversion_ms = symbol_stats.last_timestamp - timestamp;
            symbol_stats.inversion_distances.push(inversion_ms);
            
            if inversion_ms > symbol_stats.max_inversion_ms {
                symbol_stats.max_inversion_ms = inversion_ms;
            }
        }

        // Check for sequence gaps
        if sequence > 0 && symbol_stats.last_sequence > 0 {
            let expected_seq = symbol_stats.last_sequence + 1;
            if sequence != expected_seq {
                exchange_stats.packet_gaps += 1;
                let gap_size = if sequence > expected_seq {
                    sequence - expected_seq
                } else {
                    0
                };
                
                if gap_size > exchange_stats.max_gap_size {
                    exchange_stats.max_gap_size = gap_size;
                }
            }
        }

        symbol_stats.last_timestamp = timestamp;
        symbol_stats.last_sequence = sequence;
        symbol_stats.last_update = Instant::now();
    }

    async fn display_stats(&self) {
        let exchanges = self.exchanges.lock().await;
        
        // Clear screen and move cursor to top
        print!("\x1B[2J\x1B[1;1H");
        
        println!("{}", "═══════════════════════════════════════════════════════════════════════".bright_cyan());
        println!("{}", "                     ORDER VALIDATION MONITOR                          ".bright_white().bold());
        println!("{}", "═══════════════════════════════════════════════════════════════════════".bright_cyan());
        println!();
        
        let now = Local::now();
        println!("📊 {} | Refresh: {}s", 
            now.format("%Y-%m-%d %H:%M:%S").to_string().bright_yellow(),
            self.display_interval.as_secs()
        );
        println!();

        if exchanges.is_empty() {
            println!("{}", "⏳ Waiting for data from feeder...".yellow());
            println!();
            println!("  Make sure:");
            println!("  1. The feeder is running: {}", "cargo run --bin feeder".bright_cyan());
            println!("  2. Multicast address matches: {}", "239.255.0.1:9001".bright_cyan());
            println!("  3. Firewall allows UDP multicast traffic");
            println!();
            println!("  No packets received yet. Will keep checking...");
            return;
        }

        for (exchange, stats) in exchanges.iter() {
            let uptime = stats.start_time.elapsed().as_secs();
            let uptime_str = format_duration(uptime);
            
            println!("{} {} {}", 
                "Exchange:".bright_green(),
                exchange.bright_white().bold(),
                format!("[{}]", uptime_str).bright_blue()
            );
            
            println!("  📦 Total Packets: {}", 
                stats.total_packets.to_string().cyan()
            );
            
            if stats.total_inversions > 0 {
                let inversion_rate = (stats.total_inversions as f64 / stats.total_packets as f64) * 100.0;
                println!("  ⚠️  Timestamp Inversions: {} ({:.3}%)", 
                    stats.total_inversions.to_string().yellow(),
                    inversion_rate
                );
            } else {
                println!("  ✅ Timestamp Inversions: {} (0%)", 
                    "0".green()
                );
            }
            
            if stats.packet_gaps > 0 {
                println!("  🔸 Sequence Gaps: {} (max gap: {})", 
                    stats.packet_gaps.to_string().yellow(),
                    stats.max_gap_size.to_string().red()
                );
            }
            
            // Show top symbols with issues
            let mut problematic_symbols: Vec<_> = stats.symbols.iter()
                .filter(|(_, s)| s.inversions > 0)
                .collect();
            
            if !problematic_symbols.is_empty() {
                problematic_symbols.sort_by(|a, b| b.1.inversions.cmp(&a.1.inversions));
                
                println!("\n  {} Top Symbols with Order Issues:", "⚡".yellow());
                for (symbol, symbol_stats) in problematic_symbols.iter().take(5) {
                    let inversion_rate = (symbol_stats.inversions as f64 / symbol_stats.total_packets as f64) * 100.0;
                    let avg_inversion = if !symbol_stats.inversion_distances.is_empty() {
                        let sum: i64 = symbol_stats.inversion_distances.iter().sum();
                        sum / symbol_stats.inversion_distances.len() as i64
                    } else {
                        0
                    };
                    
                    println!("    {} {}: {} inversions ({:.2}%), max: {}ms, avg: {}ms",
                        "•".bright_blue(),
                        symbol.bright_white(),
                        symbol_stats.inversions.to_string().yellow(),
                        inversion_rate,
                        symbol_stats.max_inversion_ms.to_string().red(),
                        avg_inversion.to_string().yellow()
                    );
                }
            }
            
            // Calculate overall health score
            let health_score = calculate_health_score(stats);
            let health_bar = create_health_bar(health_score);
            println!("\n  {} Health Score: {} {:.1}%", 
                "💚",
                health_bar,
                health_score
            );
            
            println!();
        }
        
        // Summary statistics
        let total_packets: u64 = exchanges.values().map(|e| e.total_packets).sum();
        let total_inversions: u64 = exchanges.values().map(|e| e.total_inversions).sum();
        let total_gaps: u64 = exchanges.values().map(|e| e.packet_gaps).sum();
        
        println!("{}", "─────────────────────────────────────────────────────────────────────".bright_blue());
        println!("{} Total Packets: {} | Inversions: {} | Gaps: {}",
            "📈",
            total_packets.to_string().bright_cyan(),
            if total_inversions > 0 { 
                total_inversions.to_string().yellow() 
            } else { 
                total_inversions.to_string().green() 
            },
            if total_gaps > 0 { 
                total_gaps.to_string().yellow() 
            } else { 
                total_gaps.to_string().green() 
            }
        );
        
        if total_packets > 0 {
            let overall_inversion_rate = (total_inversions as f64 / total_packets as f64) * 100.0;
            println!("Overall Order Quality: {}", 
                if overall_inversion_rate < 0.1 {
                    format!("EXCELLENT ({:.4}% out-of-order)", overall_inversion_rate).green()
                } else if overall_inversion_rate < 1.0 {
                    format!("GOOD ({:.3}% out-of-order)", overall_inversion_rate).yellow()
                } else {
                    format!("POOR ({:.2}% out-of-order)", overall_inversion_rate).red()
                }
            );
        }
    }
}

fn calculate_health_score(stats: &ExchangeOrderStats) -> f64 {
    if stats.total_packets == 0 {
        return 100.0;
    }
    
    let inversion_rate = (stats.total_inversions as f64 / stats.total_packets as f64) * 100.0;
    let gap_rate = (stats.packet_gaps as f64 / stats.total_packets as f64) * 100.0;
    
    // Score calculation: start at 100, subtract penalties
    let mut score = 100.0;
    score -= inversion_rate * 10.0;  // Heavy penalty for inversions
    score -= gap_rate * 5.0;          // Medium penalty for gaps
    
    score.max(0.0).min(100.0)
}

fn create_health_bar(score: f64) -> String {
    let filled = (score / 10.0) as usize;
    let empty = 10 - filled;
    
    let color = if score >= 90.0 {
        "green"
    } else if score >= 70.0 {
        "yellow"
    } else {
        "red"
    };
    
    let bar = format!("{}{}",
        "█".repeat(filled),
        "░".repeat(empty)
    );
    
    match color {
        "green" => bar.green().to_string(),
        "yellow" => bar.yellow().to_string(),
        "red" => bar.red().to_string(),
        _ => bar,
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "Starting Order Validation Monitor...".bright_green().bold());
    
    // Bind to multicast
    let multicast_addr = "239.255.0.1";
    let port = 9001;
    let socket_addr = format!("0.0.0.0:{}", port);
    
    let socket = UdpSocket::bind(&socket_addr).await?;
    println!("✓ Listening on {}", socket_addr.bright_cyan());
    
    // Join multicast group
    let multicast_ip: Ipv4Addr = multicast_addr.parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_ip, interface)?;
    println!("✓ Joined multicast group {}", multicast_addr.bright_cyan());
    
    let validator = OrderValidator::new();
    let mut buf = vec![0u8; 65536];
    
    println!("{}", "─────────────────────────────────────────────────────────────────────".bright_blue());
    println!("Monitoring packet order quality. Stats will update every 5 seconds...\n");
    
    // Spawn display task that runs independently
    let validator_clone = validator.clone();
    tokio::spawn(async move {
        let mut display_interval = tokio::time::interval(Duration::from_secs(5));
        display_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        
        loop {
            display_interval.tick().await;
            validator_clone.display_stats().await;
        }
    });
    
    // Add packet counter for debugging
    let mut total_packets_received = 0u64;
    let mut last_packet_log = Instant::now();
    
    loop {
        // Use timeout to detect if we're receiving data
        match tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                total_packets_received += 1;
                
                // Log first few packets for debugging
                if total_packets_received <= 5 || last_packet_log.elapsed() > Duration::from_secs(30) {
                    println!("📦 Received packet #{} from {} ({} bytes)", 
                        total_packets_received, addr, len);
                    last_packet_log = Instant::now();
                }
                
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    // Log packet type for debugging
                    if total_packets_received <= 5 {
                        let packet_type = data.split('|').next().unwrap_or("UNKNOWN");
                        println!("   Type: {}", packet_type);
                    }
                    validator.process_packet(data).await;
                }
            }
            Ok(Err(e)) => {
                eprintln!("❌ Error receiving packet: {}", e);
            }
            Err(_) => {
                // Timeout - no packets received
                // This is normal if no feeder is running
            }
        }
    }
}