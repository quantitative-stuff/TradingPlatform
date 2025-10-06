use std::time::Duration;
use tokio::time::interval;
use chrono::{DateTime, Utc, Local};
use colored::*;
use std::collections::HashMap;
use anyhow::Result;

use feeder::connect_to_databse::{QuestDBClient, QuestDBConfig};

#[derive(Debug, Clone)]
struct ExchangeMetrics {
    exchange: String,
    cpu_usage: f64,
    memory_mb: f64,
    websocket_count: u32,
    data_rate: f64,
    latency_ms: f64,
    last_update: DateTime<Utc>,
    connection_events: Vec<String>,
    recent_errors: u32,
}

struct PerformanceMonitor {
    questdb: QuestDBClient,
    metrics: HashMap<String, ExchangeMetrics>,
    refresh_interval: Duration,
}

impl PerformanceMonitor {
    async fn new() -> Result<Self> {
        // Connect to QuestDB
        let config = QuestDBConfig::default();
        let questdb = QuestDBClient::new(config).await?;
        
        Ok(Self {
            questdb,
            metrics: HashMap::new(),
            refresh_interval: Duration::from_secs(2), // Update every 2 seconds
        })
    }
    
    async fn fetch_latest_metrics(&mut self) -> Result<()> {
        // Query QuestDB for latest performance metrics
        // In production, this would use HTTP API at port 9000
        
        // For now, simulate with placeholder
        for exchange in ["Binance", "Bybit", "Upbit"] {
            self.metrics.insert(
                exchange.to_string(),
                ExchangeMetrics {
                    exchange: exchange.to_string(),
                    cpu_usage: 45.2 + rand::random::<f64>() * 10.0,
                    memory_mb: 512.0 + rand::random::<f64>() * 50.0,
                    websocket_count: 20,
                    data_rate: 1500.0 + rand::random::<f64>() * 200.0,
                    latency_ms: 1.0 + rand::random::<f64>() * 2.0,
                    last_update: Utc::now(),
                    connection_events: vec![],
                    recent_errors: (rand::random::<f64>() * 5.0) as u32,
                }
            );
        }
        
        Ok(())
    }
    
    fn render_dashboard(&self) {
        // Clear screen
        print!("\x1B[2J\x1B[1;1H");
        
        // Header
        println!("{}", "═══════════════════════════════════════════════════════════════════".bright_blue());
        println!("{}", "                    FEEDER PERFORMANCE MONITOR                      ".bright_white().bold());
        println!("{}", "═══════════════════════════════════════════════════════════════════".bright_blue());
        println!();
        
        // Current time
        println!("📅 {}", Local::now().format("%Y-%m-%d %H:%M:%S").to_string().bright_cyan());
        println!();
        
        // Performance table header
        println!("{}", "┌─────────────┬──────────┬──────────┬───────────┬────────────┬──────────┬─────────┐".bright_white());
        println!("{}", "│  Exchange   │   CPU %  │ Memory MB│ WebSockets│  Msgs/sec  │ Latency  │ Errors  │".bright_white());
        println!("{}", "├─────────────┼──────────┼──────────┼───────────┼────────────┼──────────┼─────────┤".bright_white());
        
        // Metrics for each exchange
        for exchange in ["Binance", "Bybit", "Upbit"] {
            if let Some(metrics) = self.metrics.get(exchange) {
                let cpu_color = if metrics.cpu_usage > 70.0 { "red" } 
                                else if metrics.cpu_usage > 50.0 { "yellow" } 
                                else { "green" };
                                
                let latency_color = if metrics.latency_ms > 5.0 { "red" }
                                   else if metrics.latency_ms > 2.0 { "yellow" }
                                   else { "green" };
                                   
                let error_color = if metrics.recent_errors > 0 { "red" } else { "green" };
                
                println!("│ {:<11} │ {:>8.1} │ {:>8.1} │ {:>9} │ {:>10.1} │ {:>7.2}ms│ {:>7} │",
                    exchange.bright_cyan(),
                    format!("{:.1}", metrics.cpu_usage).color(cpu_color),
                    metrics.memory_mb,
                    metrics.websocket_count.to_string().bright_white(),
                    metrics.data_rate,
                    format!("{:.2}", metrics.latency_ms).color(latency_color),
                    metrics.recent_errors.to_string().color(error_color),
                );
            }
        }
        
        println!("{}", "└─────────────┴──────────┴──────────┴───────────┴────────────┴──────────┴─────────┘".bright_white());
        println!();
        
        // System summary
        let total_cpu: f64 = self.metrics.values().map(|m| m.cpu_usage).sum();
        let total_memory: f64 = self.metrics.values().map(|m| m.memory_mb).sum();
        let total_messages: f64 = self.metrics.values().map(|m| m.data_rate).sum();
        let total_connections: u32 = self.metrics.values().map(|m| m.websocket_count).sum();
        
        println!("{}", "═══════════════════════════════════════════════════════════════════".bright_blue());
        println!("📊 {} ", "SYSTEM TOTALS".bright_yellow().bold());
        println!("   CPU Usage:    {:.1}%", total_cpu / 3.0);
        println!("   Memory:       {:.1} MB", total_memory);
        println!("   Connections:  {}", total_connections);
        println!("   Throughput:   {:.0} msgs/sec", total_messages);
        println!("{}", "═══════════════════════════════════════════════════════════════════".bright_blue());
        
        // Status indicators
        println!();
        self.print_status_bar();
        
        // Instructions
        println!();
        println!("{}", "Press Ctrl+C to exit".dimmed());
    }
    
    fn print_status_bar(&self) {
        print!("Status: ");
        
        let all_healthy = self.metrics.values().all(|m| 
            m.cpu_usage < 70.0 && 
            m.latency_ms < 5.0 && 
            m.recent_errors == 0
        );
        
        if all_healthy {
            println!("{} All systems operational", "●".green());
        } else {
            let issues: Vec<String> = self.metrics.values()
                .filter_map(|m| {
                    if m.cpu_usage > 70.0 {
                        Some(format!("{}: High CPU", m.exchange))
                    } else if m.latency_ms > 5.0 {
                        Some(format!("{}: High latency", m.exchange))
                    } else if m.recent_errors > 0 {
                        Some(format!("{}: {} errors", m.exchange, m.recent_errors))
                    } else {
                        None
                    }
                })
                .collect();
                
            println!("{} Issues: {}", "●".yellow(), issues.join(", ").yellow());
        }
    }
    
    async fn run(&mut self) -> Result<()> {
        let mut interval = interval(self.refresh_interval);
        
        loop {
            interval.tick().await;
            
            // Fetch latest metrics from QuestDB
            self.fetch_latest_metrics().await?;
            
            // Render dashboard
            self.render_dashboard();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting Performance Monitor...");
    
    let mut monitor = PerformanceMonitor::new().await?;
    
    // Handle Ctrl+C gracefully
    tokio::select! {
        result = monitor.run() => {
            if let Err(e) = result {
                eprintln!("Monitor error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down monitor...");
        }
    }
    
    Ok(())
}

// Add rand for demo purposes
use rand;

// QuestDB HTTP Query Module (for production)
mod questdb_query {
    use reqwest;
    use serde_json::Value;
    
    pub async fn query_latest_metrics(host: &str, port: u16) -> Result<Vec<Value>, reqwest::Error> {
        let query = r#"
            SELECT exchange, 
                   avg(cpu_usage) as cpu_usage,
                   avg(memory_mb) as memory_mb,
                   avg(websocket_count) as websocket_count,
                   avg(data_rate) as data_rate,
                   avg(latency_ms) as latency_ms
            FROM feeder_performance_logs
            WHERE timestamp > dateadd('s', -10, now())
            GROUP BY exchange
        "#;
        
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{}:{}/exec", host, port))
            .query(&[("query", query)])
            .send()
            .await?
            .json::<Value>()
            .await?;
            
        Ok(vec![response])
    }
    
    pub async fn query_recent_errors(host: &str, port: u16) -> Result<Vec<Value>, reqwest::Error> {
        let query = r#"
            SELECT exchange, count() as error_count
            FROM feeder_error_logs
            WHERE timestamp > dateadd('m', -5, now())
            GROUP BY exchange
        "#;
        
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{}:{}/exec", host, port))
            .query(&[("query", query)])
            .send()
            .await?
            .json::<Value>()
            .await?;
            
        Ok(vec![response])
    }
}