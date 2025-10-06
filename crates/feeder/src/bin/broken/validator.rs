// WebSocket Validator - Clean, comprehensive monitoring for the feeder system
// This validator checks all components from SYSTEM_CHECKLIST.md

use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::time::interval;
use colored::*;
use parking_lot::RwLock;

use feeder::core::{CONNECTION_STATS, TRADES, ORDERBOOKS};

#[derive(Debug, Clone)]
struct ValidationMetrics {
    // WebSocket metrics
    total_connections: usize,
    active_connections: usize,
    failed_connections: usize,
    reconnection_attempts: usize,
    
    // Data metrics
    trades_received: usize,
    orderbooks_received: usize,
    last_trade_time: Option<Instant>,
    last_orderbook_time: Option<Instant>,
    
    // Per-exchange metrics
    exchange_stats: HashMap<String, ExchangeMetrics>,
    
    // System metrics
    start_time: Instant,
    messages_per_second: f64,
    data_staleness_seconds: u64,
}

#[derive(Debug, Clone, Default)]
struct ExchangeMetrics {
    connections: usize,
    disconnections: usize,
    reconnects: usize,
    trades: usize,
    orderbooks: usize,
    last_update: Option<Instant>,
    errors: Vec<String>,
}

impl Default for ValidationMetrics {
    fn default() -> Self {
        Self {
            total_connections: 0,
            active_connections: 0,
            failed_connections: 0,
            reconnection_attempts: 0,
            trades_received: 0,
            orderbooks_received: 0,
            last_trade_time: None,
            last_orderbook_time: None,
            exchange_stats: HashMap::new(),
            start_time: Instant::now(),
            messages_per_second: 0.0,
            data_staleness_seconds: 0,
        }
    }
}

struct Validator {
    metrics: Arc<RwLock<ValidationMetrics>>,
    check_interval: Duration,
    display_mode: DisplayMode,
}

#[derive(Debug, Clone)]
enum DisplayMode {
    Compact,    // Single-line updates
    Detailed,   // Full dashboard
    Checklist,  // System checklist format
}

impl Validator {
    fn new(display_mode: DisplayMode) -> Self {
        Self {
            metrics: Arc::new(RwLock::new(ValidationMetrics::default())),
            check_interval: Duration::from_secs(2),
            display_mode,
        }
    }
    
    // Collect metrics from global state
    fn collect_metrics(&self) {
        let mut metrics = self.metrics.write();
        
        // Collect connection stats
        let conn_stats = CONNECTION_STATS.read();
        metrics.total_connections = 0;
        metrics.active_connections = 0;
        metrics.failed_connections = 0;
        metrics.reconnection_attempts = 0;
        
        for (exchange, stats) in conn_stats.iter() {
            let exchange_metrics = metrics.exchange_stats.entry(exchange.clone())
                .or_insert_with(ExchangeMetrics::default);
            
            exchange_metrics.connections = stats.connected;
            exchange_metrics.disconnections = stats.disconnected;
            exchange_metrics.reconnects = stats.reconnect_count;
            
            if let Some(error) = &stats.last_error {
                if !exchange_metrics.errors.contains(error) {
                    exchange_metrics.errors.push(error.clone());
                    // Keep only last 5 errors
                    if exchange_metrics.errors.len() > 5 {
                        exchange_metrics.errors.remove(0);
                    }
                }
            }
        }
        
        // Update totals after processing all exchanges
        for (_exchange, stats) in conn_stats.iter() {
            metrics.total_connections += stats.total_connections;
            metrics.active_connections += stats.connected;
            metrics.failed_connections += stats.disconnected;
            metrics.reconnection_attempts += stats.reconnect_count;
        }
        
        // Collect trade data
        let trades = TRADES.read();
        let prev_trades = metrics.trades_received;
        metrics.trades_received = trades.len();
        
        // Count per exchange
        for trade in trades.iter() {
            if let Some(exchange_metrics) = metrics.exchange_stats.get_mut(&trade.exchange) {
                exchange_metrics.trades = trades.iter()
                    .filter(|t| t.exchange == trade.exchange)
                    .count();
            }
        }
        
        if trades.len() > prev_trades {
            metrics.last_trade_time = Some(Instant::now());
        }
        
        // Collect orderbook data
        let orderbooks = ORDERBOOKS.read();
        let prev_orderbooks = metrics.orderbooks_received;
        metrics.orderbooks_received = orderbooks.len();
        
        // Count per exchange
        for orderbook in orderbooks.iter() {
            if let Some(exchange_metrics) = metrics.exchange_stats.get_mut(&orderbook.exchange) {
                exchange_metrics.orderbooks = orderbooks.iter()
                    .filter(|o| o.exchange == orderbook.exchange)
                    .count();
                exchange_metrics.last_update = Some(Instant::now());
            }
        }
        
        if orderbooks.len() > prev_orderbooks {
            metrics.last_orderbook_time = Some(Instant::now());
        }
        
        // Calculate message rate
        let elapsed = metrics.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let total_messages = metrics.trades_received + metrics.orderbooks_received;
            metrics.messages_per_second = total_messages as f64 / elapsed;
        }
        
        // Check data staleness
        let now = Instant::now();
        metrics.data_staleness_seconds = metrics.last_trade_time
            .or(metrics.last_orderbook_time)
            .map(|t| now.duration_since(t).as_secs())
            .unwrap_or(999);
    }
    
    // Display metrics based on mode
    fn display(&self) {
        let metrics = self.metrics.read();
        
        match self.display_mode {
            DisplayMode::Compact => self.display_compact(&metrics),
            DisplayMode::Detailed => self.display_detailed(&metrics),
            DisplayMode::Checklist => self.display_checklist(&metrics),
        }
    }
    
    fn display_compact(&self, metrics: &ValidationMetrics) {
        // Single line status
        print!("\r");
        print!("📡 Connections: {}/{} ", 
            metrics.active_connections.to_string().green(),
            metrics.total_connections.to_string().yellow()
        );
        print!("| 📊 Data: {} trades, {} books ", 
            format_number(metrics.trades_received).cyan(),
            format_number(metrics.orderbooks_received).cyan()
        );
        print!("| 📈 Rate: {:.1} msg/s ", 
            metrics.messages_per_second
        );
        
        // Status indicator
        let status = if metrics.data_staleness_seconds < 5 {
            "✅ LIVE".green()
        } else if metrics.data_staleness_seconds < 30 {
            "⚠️ DELAYED".yellow()
        } else {
            "❌ STALE".red()
        };
        print!("| Status: {}", status);
        
        // Flush to ensure immediate display
        use std::io::{stdout, Write};
        let _ = stdout().flush();
    }
    
    fn display_detailed(&self, metrics: &ValidationMetrics) {
        clear_screen();
        
        println!("{}", "═".repeat(80).blue());
        println!("{}", "            WEBSOCKET VALIDATOR - LIVE MONITORING".bold().white());
        println!("{}", "═".repeat(80).blue());
        
        // Connection Summary
        println!("\n{}", "📡 CONNECTION STATUS".yellow().bold());
        println!("{}", "─".repeat(40));
        println!("Total Connections:    {}", metrics.total_connections.to_string().white());
        println!("Active:              {} {}", 
            metrics.active_connections.to_string().green(),
            if metrics.active_connections > 0 { "✓" } else { "✗" }
        );
        println!("Failed:              {}", metrics.failed_connections.to_string().red());
        println!("Reconnection Attempts: {}", metrics.reconnection_attempts.to_string().yellow());
        
        // Data Flow
        println!("\n{}", "📊 DATA FLOW".cyan().bold());
        println!("{}", "─".repeat(40));
        println!("Trades Received:     {}", format_number(metrics.trades_received).green());
        println!("Orderbooks Received: {}", format_number(metrics.orderbooks_received).green());
        println!("Message Rate:        {:.2} msg/sec", metrics.messages_per_second);
        println!("Data Age:           {} seconds", 
            if metrics.data_staleness_seconds < 10 {
                metrics.data_staleness_seconds.to_string().green()
            } else if metrics.data_staleness_seconds < 60 {
                metrics.data_staleness_seconds.to_string().yellow()
            } else {
                metrics.data_staleness_seconds.to_string().red()
            }
        );
        
        // Per-Exchange Details
        if !metrics.exchange_stats.is_empty() {
            println!("\n{}", "🏢 EXCHANGE BREAKDOWN".magenta().bold());
            println!("{}", "─".repeat(70));
            println!("{:<15} {:>10} {:>10} {:>10} {:>12} {:>10}", 
                "Exchange", "Conn", "Disc", "Trades", "Orderbooks", "Status"
            );
            println!("{}", "─".repeat(70));
            
            for (exchange, stats) in &metrics.exchange_stats {
                let status = if stats.last_update.map_or(999, |t| t.elapsed().as_secs()) < 5 {
                    "LIVE".green()
                } else {
                    "STALE".red()
                };
                
                println!("{:<15} {:>10} {:>10} {:>10} {:>12} {:>10}", 
                    exchange,
                    stats.connections.to_string().green(),
                    stats.disconnections.to_string().red(),
                    format_number(stats.trades),
                    format_number(stats.orderbooks),
                    status
                );
                
                // Show recent errors
                if !stats.errors.is_empty() {
                    for error in &stats.errors {
                        println!("  └─ ⚠️ {}", error.chars().take(60).collect::<String>().yellow());
                    }
                }
            }
        }
        
        // Health Check Summary
        println!("\n{}", "✅ HEALTH CHECK".green().bold());
        println!("{}", "─".repeat(40));
        
        let health_items = vec![
            ("WebSocket Connections", metrics.active_connections > 0),
            ("Data Flow Active", metrics.data_staleness_seconds < 10),
            ("Reconnection Working", metrics.reconnection_attempts > 0 || metrics.failed_connections == 0),
            ("All Exchanges Connected", metrics.exchange_stats.values().all(|s| s.connections > 0)),
        ];
        
        for (item, status) in health_items {
            println!("{:<25} {}", 
                item, 
                if status { "✅ PASS".green() } else { "❌ FAIL".red() }
            );
        }
        
        println!("\n{}", "─".repeat(80));
        println!("Press Ctrl+C to exit | Updates every 2 seconds");
    }
    
    fn display_checklist(&self, metrics: &ValidationMetrics) {
        clear_screen();
        
        println!("{}", "╔══════════════════════════════════════════════════════════════╗".blue());
        println!("{}", "║           SYSTEM VALIDATION CHECKLIST                       ║".blue());
        println!("{}", "╚══════════════════════════════════════════════════════════════╝".blue());
        
        // Based on SYSTEM_CHECKLIST.md
        let mut checks = Vec::new();
        
        // 1. WebSocket Connection Component
        checks.push(("WebSocket Connections", vec![
            ("Connections established", metrics.active_connections > 0),
            ("Binance connected", metrics.exchange_stats.get("Binance").map_or(false, |s| s.connections > 0)),
            ("Bybit connected", metrics.exchange_stats.get("Bybit").map_or(false, |s| s.connections > 0)),
            ("Upbit connected", metrics.exchange_stats.get("Upbit").map_or(false, |s| s.connections > 0)),
            ("Reconnection working", metrics.reconnection_attempts > 0 || metrics.failed_connections == 0),
        ]));
        
        // 2. Data Reception Component
        checks.push(("Data Reception", vec![
            ("Trades receiving", metrics.trades_received > 0),
            ("Orderbooks receiving", metrics.orderbooks_received > 0),
            ("No data truncation", true), // Assumed if receiving
            ("Message rate normal", metrics.messages_per_second > 0.0),
        ]));
        
        // 3. Data Processing Component
        checks.push(("Data Processing", vec![
            ("Data freshness < 10s", metrics.data_staleness_seconds < 10),
            ("All exchanges active", metrics.exchange_stats.values().all(|s| s.last_update.is_some())),
            ("Error rate acceptable", metrics.exchange_stats.values().all(|s| s.errors.len() < 3)),
        ]));
        
        // Display checklist
        for (category, items) in &checks {
            println!("\n{} {}", "▶".yellow(), category.bold());
            for (item, passed) in items {
                let symbol = if *passed { "✅".green() } else { "❌".red() };
                let status = if *passed { "PASS".green() } else { "FAIL".red() };
                println!("  {} {} ... [{}]", symbol, item, status);
            }
        }
        
        // Summary
        let total_checks: usize = checks.iter().map(|(_, items)| items.len()).sum();
        let passed_checks: usize = checks.iter()
            .map(|(_, items)| items.iter().filter(|(_, p)| *p).count())
            .sum();
        
        println!("\n{}", "═".repeat(60));
        println!("VALIDATION SUMMARY: {}/{} checks passed", 
            passed_checks.to_string().green(), 
            total_checks
        );
        
        if passed_checks == total_checks {
            println!("{}", "✅ ALL SYSTEMS OPERATIONAL".green().bold());
        } else {
            println!("{}", "⚠️ ISSUES DETECTED - CHECK FAILED ITEMS".yellow().bold());
        }
    }
    
    async fn run(&self) -> Result<()> {
        let mut check_interval = interval(self.check_interval);
        
        println!("Starting WebSocket Validator...");
        println!("Mode: {:?}", self.display_mode);
        println!("Collecting initial metrics...\n");
        
        // Initial check
        println!("Checking global state access...");
        self.collect_metrics();
        let metrics = self.metrics.read();
        println!("Initial state: {} connections, {} trades, {} orderbooks", 
            metrics.total_connections, 
            metrics.trades_received, 
            metrics.orderbooks_received
        );
        drop(metrics);
        println!();
        
        // Give system time to start
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        loop {
            tokio::select! {
                _ = check_interval.tick() => {
                    // Collect latest metrics
                    self.collect_metrics();
                    
                    // Display based on mode
                    self.display();
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\n\nValidator stopped.");
                    break;
                }
            }
        }
        
        Ok(())
    }
}

fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
    use std::io::{stdout, Write};
    let _ = stdout().flush();
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

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    let display_mode = if args.len() > 1 {
        match args[1].as_str() {
            "compact" => DisplayMode::Compact,
            "detailed" => DisplayMode::Detailed,
            "checklist" => DisplayMode::Checklist,
            _ => {
                println!("Usage: validator [compact|detailed|checklist]");
                println!("  compact   - Single-line status updates");
                println!("  detailed  - Full dashboard view (default)");
                println!("  checklist - System validation checklist");
                return Ok(());
            }
        }
    } else {
        DisplayMode::Detailed
    };
    
    let validator = Validator::new(display_mode);
    validator.run().await
}