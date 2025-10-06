use anyhow::Result;
use tokio::net::UdpSocket;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Instant;
use std::collections::{HashMap, HashSet};
use tokio::time::{interval, Duration};
use tracing::{info, warn, error};
use sysinfo::System;
use parking_lot::RwLock;

// use feeder::monitoring::performance::{PERFORMANCE_MONITOR, PerformanceMetrics};
// use feeder::monitoring::alerts::{ALERT_MANAGER, Alert, AlertSeverity};
use feeder::core::{setup_console_logging, CONNECTION_STATS, TRADES, ORDERBOOKS};

#[derive(Debug, Clone)]
struct ExchangeStats {
    trades_count: usize,
    orderbooks_count: usize,
    last_update: Instant,
}

impl Default for ExchangeStats {
    fn default() -> Self {
        Self {
            trades_count: 0,
            orderbooks_count: 0,
            last_update: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
struct ConnectionStats {
    connected: usize,
    disconnected: usize,
    reconnect_count: usize,
    total_attempts: usize,
}

#[derive(Debug)]
struct UdpStats {
    exchanges: HashMap<String, ExchangeStats>,
    connections: HashMap<String, ConnectionStats>,
    packets_received: usize,
    bytes_received: usize,
    last_update: Instant,
}

struct SystemMonitor {
    system: Arc<RwLock<System>>,
}

impl SystemMonitor {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        
        Self {
            system: Arc::new(RwLock::new(system)),
        }
    }
    
    fn get_cpu_usage(&self) -> f64 {
        let mut system = self.system.write();
        system.refresh_cpu_specifics(sysinfo::CpuRefreshKind::everything());
        
        let cpus = system.cpus();
        if cpus.is_empty() {
            return 0.0;
        }
        
        let total_usage: f32 = cpus.iter().map(|cpu| cpu.cpu_usage()).sum();
        (total_usage / cpus.len() as f32) as f64
    }
    
    fn get_memory_usage_mb(&self) -> f64 {
        let mut system = self.system.write();
        system.refresh_memory();
        
        let pid = sysinfo::Pid::from_u32(std::process::id());
        if let Some(process) = system.process(pid) {
            return process.memory() as f64 / 1024.0 / 1024.0;
        }
        
        (system.used_memory() as f64) / 1024.0 / 1024.0
    }
}

async fn start_udp_listener(udp_stats: Arc<Mutex<UdpStats>>) {
    tokio::spawn(async move {
        // Bind to 0.0.0.0:9001 to accept packets from any IP address
        let socket = match UdpSocket::bind("0.0.0.0:9001").await {
            Ok(s) => {
                eprintln!("Monitor: UDP socket bound to 0.0.0.0:9001 (accepting from any IP)");
                eprintln!("Monitor: Socket configured, waiting for packets...");
                s
            }
            Err(e) => {
                eprintln!("Monitor: CRITICAL - Failed to bind UDP socket on 127.0.0.1:9001: {}", e);
                eprintln!("Monitor: Is another process using port 9001?");
                return;
            }
        };
        
        let mut buf = [0; 65536];
        let mut packet_count = 0;

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((size, addr)) => {
                    packet_count += 1;
                    if packet_count <= 5 {
                        eprintln!("Monitor: Received packet #{} from {} ({} bytes)", packet_count, addr, size);
                    }
                    
                    if let Ok(packet_str) = std::str::from_utf8(&buf[..size]) {
                        if packet_count <= 5 {
                            eprintln!("Monitor: Packet content: {}", packet_str);
                        }
                        
                        let parts: Vec<&str> = packet_str.split('|').collect();
                        
                        // EXCHANGE|exchange_name|asset_type|trades|orderbooks|timestamp
                        if parts.len() >= 5 && parts[0] == "EXCHANGE" {
                            let exchange_name = parts[1];
                            let asset_type = parts[2];
                            let trades = parts[3].parse::<usize>().unwrap_or(0);
                            let orderbooks = parts[4].parse::<usize>().unwrap_or(0);
                            
                            let key = format!("{}_{}", exchange_name, asset_type);
                            
                            let mut stats = udp_stats.lock().await;
                            let exchange_stats = stats.exchanges.entry(key).or_insert(ExchangeStats::default());
                            exchange_stats.trades_count = trades;
                            exchange_stats.orderbooks_count = orderbooks;
                            exchange_stats.last_update = Instant::now();
                            
                            stats.packets_received += 1;
                            stats.bytes_received += size;
                            stats.last_update = Instant::now();
                        }
                        // CONN|exchange|connected|disconnected|reconnect_count|total_connections
                        else if parts.len() >= 4 && parts[0] == "CONN" {
                            let exchange = parts[1];
                            let connected = parts[2].parse::<usize>().unwrap_or(0);
                            let disconnected = parts[3].parse::<usize>().unwrap_or(0);
                            let reconnect_count = parts.get(4)
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(0);
                            let total_attempts = parts.get(5)
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(0);
                            
                            let mut stats = udp_stats.lock().await;
                            stats.connections.insert(
                                exchange.to_string(),
                                ConnectionStats {
                                    connected,
                                    disconnected,
                                    reconnect_count,
                                    total_attempts,
                                }
                            );
                            stats.packets_received += 1;
                            stats.bytes_received += size;
                            stats.last_update = Instant::now();
                        }
                        // Legacy format: STATS|trades|orderbooks
                        else if parts.len() >= 3 && parts[0] == "STATS" {
                            let trades = parts[1].parse::<usize>().unwrap_or(0);
                            let orderbooks = parts[2].parse::<usize>().unwrap_or(0);
                            
                            let mut stats = udp_stats.lock().await;
                            let exchange_stats = stats.exchanges.entry("Total".to_string())
                                .or_insert(ExchangeStats::default());
                            exchange_stats.trades_count = trades;
                            exchange_stats.orderbooks_count = orderbooks;
                            exchange_stats.last_update = Instant::now();
                            
                            stats.packets_received += 1;
                            stats.bytes_received += size;
                            stats.last_update = Instant::now();
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Monitor: UDP receive error: {}", e);
                    eprintln!("Monitor: Still listening on port 9001...");
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    });
}

fn display_real_connection_stats() {
    println!("\n📡 WEBSOCKET CONNECTIONS (Live from CONNECTION_STATS):");
    println!("{}", "=".repeat(70));
    
    let stats = CONNECTION_STATS.read();
    
    if stats.is_empty() {
        println!("   No active connections yet...");
    } else {
        println!("Exchange    | Connected | Disconnected | Reconnects | Total | Last Error");
        println!("{}", "-".repeat(70));
        
        let mut total_connected = 0;
        let mut total_disconnected = 0;
        let mut total_reconnects = 0;
        
        for (exchange, stat) in stats.iter() {
            println!("{:<11} | {:>9} | {:>12} | {:>10} | {:>5} | {}",
                exchange,
                stat.connected,
                stat.disconnected,
                stat.reconnect_count,
                stat.total_connections,
                stat.last_error.as_ref().unwrap_or(&"None".to_string())
                    .chars().take(20).collect::<String>()
            );
            
            total_connected += stat.connected;
            total_disconnected += stat.disconnected;
            total_reconnects += stat.reconnect_count;
        }
        
        println!("{}", "-".repeat(70));
        println!("TOTAL       | {:>9} | {:>12} | {:>10} |", 
            total_connected, total_disconnected, total_reconnects);
    }
}

fn display_real_market_data() {
    println!("\n📊 MARKET DATA (Live from TRADES/ORDERBOOKS):");
    println!("{}", "=".repeat(70));
    
    let trades = TRADES.read();
    let orderbooks = ORDERBOOKS.read();
    
    // Count by exchange
    let mut trade_counts: HashMap<String, usize> = HashMap::new();
    let mut orderbook_counts: HashMap<String, usize> = HashMap::new();
    
    for trade in trades.iter() {
        *trade_counts.entry(trade.exchange.clone()).or_insert(0) += 1;
    }
    
    for ob in orderbooks.iter() {
        *orderbook_counts.entry(ob.exchange.clone()).or_insert(0) += 1;
    }
    
    if trade_counts.is_empty() && orderbook_counts.is_empty() {
        println!("   No market data received yet...");
    } else {
        println!("Exchange    | Trades (Total) | Orderbooks (Total) | Latest Symbol");
        println!("{}", "-".repeat(70));
        
        let mut all_exchanges = std::collections::HashSet::new();
        all_exchanges.extend(trade_counts.keys().cloned());
        all_exchanges.extend(orderbook_counts.keys().cloned());
        
        for exchange in all_exchanges {
            let trade_count = trade_counts.get(&exchange).unwrap_or(&0);
            let ob_count = orderbook_counts.get(&exchange).unwrap_or(&0);
            
            // Get latest symbol from this exchange
            let latest_symbol = trades.iter()
                .filter(|t| t.exchange == exchange)
                .last()
                .map(|t| t.symbol.clone())
                .or_else(|| orderbooks.iter()
                    .filter(|o| o.exchange == exchange)
                    .last()
                    .map(|o| o.symbol.clone()))
                .unwrap_or_else(|| "N/A".to_string());
            
            println!("{:<11} | {:>14} | {:>18} | {}",
                exchange,
                format_number(*trade_count),
                format_number(*ob_count),
                latest_symbol
            );
        }
        
        println!("{}", "-".repeat(70));
        println!("TOTAL       | {:>14} | {:>18} |",
            format_number(trades.len()),
            format_number(orderbooks.len())
        );
        
        // Show memory usage
        let trade_memory = trades.len() * std::mem::size_of::<market_feeder::core::TradeData>();
        let ob_memory = orderbooks.len() * std::mem::size_of::<market_feeder::core::OrderBookData>();
        println!("\n💾 Memory: Trades: {} MB | Orderbooks: {} MB",
            trade_memory / 1_048_576,
            ob_memory / 1_048_576
        );
    }
}

fn clear_terminal() {
    use std::io::{stdout, Write};
    
    // Windows-compatible terminal clearing
    #[cfg(windows)]
    {
        // For Windows, try different methods
        // Method 1: ANSI escape codes (works in Windows Terminal, modern PowerShell)
        print!("\x1B[2J\x1B[1;1H");
        // Flush to ensure it takes effect immediately
        let _ = stdout().flush();
    }
    
    #[cfg(not(windows))]
    {
        // Unix/Linux ANSI escape codes
        print!("\x1B[2J\x1B[1;1H");
        let _ = stdout().flush();
    }
}

fn print_header() {
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║                   UNIFIED MONITOR - Real-time Metrics                 ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();
}

fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn display_udp_stats(stats: &UdpStats) {
    let now = Instant::now();
    
    // Connection statistics
    if !stats.connections.is_empty() {
        println!("\n🔌 WebSocket Connections:");
        println!("{:<15} {:>10} {:>12} {:>12} {:>10}", 
            "Exchange", "Connected", "Disconnected", "Reconnects", "Total");
        println!("{}", "-".repeat(75));
        
        let mut total_connected = 0;
        let mut total_disconnected = 0;
        let mut total_reconnects = 0;
        let mut total_attempts = 0;
        
        for (exchange, conn) in &stats.connections {
            println!("{:<15} {:>10} {:>12} {:>12} {:>10}", 
                exchange,
                conn.connected,
                conn.disconnected,
                conn.reconnect_count,
                conn.total_attempts
            );
            total_connected += conn.connected;
            total_disconnected += conn.disconnected;
            total_reconnects += conn.reconnect_count;
            total_attempts += conn.total_attempts;
        }
        
        println!("{}", "-".repeat(75));
        println!("{:<15} {:>10} {:>12} {:>12} {:>10}", 
            "TOTAL",
            total_connected,
            total_disconnected,
            total_reconnects,
            total_attempts
        );
    }
    
    // Exchange statistics
    if !stats.exchanges.is_empty() {
        println!("\n📊 Exchange Statistics:");
        println!("{:<20} {:>15} {:>15} {:>10}", 
            "Exchange/Asset", "Trades", "OrderBooks", "Status");
        println!("{}", "-".repeat(70));
        
        let mut total_trades = 0;
        let mut total_orderbooks = 0;
        
        let mut exchanges: Vec<_> = stats.exchanges.iter().collect();
        exchanges.sort_by_key(|(name, _)| name.as_str());
        
        for (exchange_key, exchange_stats) in exchanges {
            let age = now.duration_since(exchange_stats.last_update).as_secs();
            let status = if age > 10 {
                format!("⚠️  {}s ago", age)
            } else if age > 5 {
                format!("🟡 {}s ago", age)
            } else {
                "✅ Active".to_string()
            };
            
            println!("{:<20} {:>15} {:>15} {:>10}", 
                exchange_key,
                format_number(exchange_stats.trades_count),
                format_number(exchange_stats.orderbooks_count),
                status
            );
            
            total_trades += exchange_stats.trades_count;
            total_orderbooks += exchange_stats.orderbooks_count;
        }
        
        println!("{}", "-".repeat(70));
        println!("{:<20} {:>15} {:>15}", 
            "TOTAL",
            format_number(total_trades),
            format_number(total_orderbooks)
        );
    }
    
    println!("\n📡 Network Stats:");
    println!("  Packets Received: {:>10}", stats.packets_received);
    println!("  Data Received:    {:>10} KB", stats.bytes_received / 1024);
    
    let age = now.duration_since(stats.last_update).as_secs();
    if age > 10 {
        println!("\n⚠️  No data received for {} seconds", age);
    } else if stats.exchanges.is_empty() {
        println!("\n⏳ Waiting for data...");
    }
}

fn display_performance_metrics(metrics: &PerformanceMetrics) {
    println!("\n🎯 Performance Metrics - {}", metrics.exchange);
    println!("{}", "-".repeat(70));
    
    // Latency
    println!("⏱️  Latency:");
    println!("   Min: {:.1}ms | Avg: {:.1}ms | Max: {:.1}ms", 
        metrics.latency_min, metrics.latency_avg, metrics.latency_max);
    println!("   P50: {:.1}ms | P95: {:.1}ms | P99: {:.1}ms", 
        metrics.latency_p50, metrics.latency_p95, metrics.latency_p99);
    
    // Throughput
    println!("📈 Throughput:");
    println!("   Messages: {:.1}/s | Bandwidth: {:.2} Mbps", 
        metrics.messages_per_second, metrics.network_bandwidth_mbps);
    
    // Resources
    println!("💻 Resources:");
    println!("   CPU: {:.1}% | Memory: {:.1} MB", 
        metrics.cpu_usage, metrics.memory_usage_mb);
    
    // Errors
    if metrics.error_rate > 0.0 || metrics.timeout_count > 0 {
        println!("⚠️  Errors:");
        println!("   Error Rate: {:.1}/min | Timeouts: {}", 
            metrics.error_rate, metrics.timeout_count);
    }
}

fn display_alerts(alerts: &[Alert]) {
    if alerts.is_empty() {
        return;
    }
    
    println!("\n🚨 ACTIVE ALERTS:");
    println!("{}", "-".repeat(70));
    
    for alert in alerts {
        let severity_icon = match alert.severity {
            AlertSeverity::Critical => "🔴",
            AlertSeverity::Error => "🟠",
            AlertSeverity::Warning => "🟡",
            AlertSeverity::Info => "🔵",
        };
        
        println!("{} [{:?}] {}", severity_icon, alert.severity, alert.title);
        println!("   {}", alert.message);
        println!("   Time: {}", alert.timestamp.format("%H:%M:%S"));
        
        if let Some(exchange) = &alert.exchange {
            println!("   Exchange: {}", exchange);
        }
    }
}

async fn run_monitor() -> Result<()> {
    // Initialize components
    let udp_stats = Arc::new(Mutex::new(UdpStats {
        exchanges: HashMap::new(),
        connections: HashMap::new(),
        packets_received: 0,
        bytes_received: 0,
        last_update: Instant::now(),
    }));
    
    let system_monitor = SystemMonitor::new();
    
    // Start UDP listener task
    start_udp_listener(udp_stats.clone()).await;
    
    // Set up intervals
    let mut display_interval = interval(Duration::from_secs(2));
    let mut perf_interval = interval(Duration::from_secs(5));
    let mut alert_interval = interval(Duration::from_secs(10));
    
    let mut known_exchanges = Vec::new();
    
    loop {
        tokio::select! {
            _ = display_interval.tick() => {
                // Clear terminal properly for Windows
                clear_terminal();
                print_header();
                
                // Display REAL WebSocket connection stats
                display_real_connection_stats();
                
                // Display REAL market data stats
                display_real_market_data();
                
                // Display UDP stats (secondary)
                let stats = udp_stats.lock().await;
                display_udp_stats(&*stats);
                drop(stats);
                
                // Update system metrics
                let cpu_usage = system_monitor.get_cpu_usage();
                let memory_usage = system_monitor.get_memory_usage_mb();
                PERFORMANCE_MONITOR.update_resource_metrics(cpu_usage, memory_usage);
            }
            
            _ = perf_interval.tick() => {
                // Get exchanges from connection stats
                let conn_stats = CONNECTION_STATS.read();
                for exchange in conn_stats.keys() {
                    if !known_exchanges.contains(exchange) {
                        known_exchanges.push(exchange.clone());
                    }
                }
                drop(conn_stats);
                
                // Display performance metrics for known exchanges
                if !known_exchanges.is_empty() {
                    for exchange in &known_exchanges {
                        let metrics = PERFORMANCE_MONITOR.calculate_metrics(exchange);
                        display_performance_metrics(&metrics);
                        ALERT_MANAGER.check_metrics(&metrics);
                    }
                }
                
                // Display alerts
                let alerts = ALERT_MANAGER.get_active_alerts();
                display_alerts(&alerts);
                
                println!("\n✅ System is running | Press Ctrl+C to exit");
            }
            
            _ = alert_interval.tick() => {
                ALERT_MANAGER.check_data_staleness();
                ALERT_MANAGER.auto_resolve_cleared_alerts();
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_console_logging("info").expect("Failed to setup logging");
    
    info!("Starting unified monitor...");
    
    // Subscribe to alerts for logging
    let mut alert_receiver = ALERT_MANAGER.subscribe();
    tokio::spawn(async move {
        while let Ok(alert) = alert_receiver.recv().await {
            match alert.severity {
                AlertSeverity::Critical => {
                    error!("🔴 CRITICAL: {}", alert.title);
                }
                AlertSeverity::Error => {
                    error!("🟠 ERROR: {}", alert.title);
                }
                AlertSeverity::Warning => {
                    warn!("🟡 WARNING: {}", alert.title);
                }
                AlertSeverity::Info => {
                    info!("🔵 INFO: {}", alert.title);
                }
            }
        }
    });
    
    run_monitor().await?;
    
    Ok(())
}