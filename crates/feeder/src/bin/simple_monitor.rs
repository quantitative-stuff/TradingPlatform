// Simple monitor - no scrolling, just updates in place
use anyhow::Result;
use std::time::Duration;
use tokio::time::interval;
use colored::*;
use feeder::core::{CONNECTION_STATS, TRADES, ORDERBOOKS};

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Simple WebSocket Monitor ===");
    println!("No scrolling - updates in place");
    println!("Press Ctrl+C to exit\n");
    
    let mut check_interval = interval(Duration::from_secs(2));
    let mut iteration = 0;
    
    // Reserve space for display (prevents scrolling)
    for _ in 0..15 {
        println!();
    }
    
    loop {
        tokio::select! {
            _ = check_interval.tick() => {
                iteration += 1;
                
                // Move cursor up 15 lines
                print!("\x1B[15A");
                
                // Clear and display header
                println!("{}", "─".repeat(60));
                println!("MONITOR UPDATE #{:<5} {}", 
                    iteration, 
                    chrono::Local::now().format("%H:%M:%S")
                );
                println!("{}", "─".repeat(60));
                
                // Connection stats
                let conn_stats = CONNECTION_STATS.read();
                println!("\n📡 CONNECTIONS:");
                
                if conn_stats.is_empty() {
                    println!("  No connections yet...");
                } else {
                    // Group by exchange name (combine spot and futures)
                    let mut exchange_totals = std::collections::HashMap::new();
                    for (key, stats) in conn_stats.iter() {
                        // Extract exchange name (before underscore)
                        let exchange = key.split('_').next().unwrap_or(key);
                        let entry = exchange_totals.entry(exchange.to_string()).or_insert((0, 0, 0));
                        entry.0 += stats.connected;
                        entry.1 += stats.disconnected;
                        entry.2 += stats.reconnect_count;
                    }
                    
                    for (exchange, (connected, disconnected, reconnects)) in exchange_totals.iter() {
                        let status = if *connected > 0 {
                            format!("{} ACTIVE", connected).green()
                        } else {
                            "DISCONNECTED".red()
                        };
                        println!("  {:<10} : {} (disc: {}, reconn: {})", 
                            exchange, 
                            status,
                            disconnected,
                            reconnects
                        );
                    }
                }
                
                // Data stats
                let trades = TRADES.read();
                let orderbooks = ORDERBOOKS.read();
                
                println!("\n📊 DATA:");
                println!("  Trades     : {}", trades.len().to_string().cyan());
                println!("  Orderbooks : {}", orderbooks.len().to_string().cyan());
                
                // Calculate rates
                let rate = if iteration > 1 {
                    (trades.len() + orderbooks.len()) as f64 / (iteration as f64 * 2.0)
                } else {
                    0.0
                };
                
                println!("  Rate       : {:.1} msg/sec", rate);
                
                // Status
                let overall_status = if !conn_stats.is_empty() && 
                    conn_stats.values().any(|s| s.connected > 0) {
                    "✅ RECEIVING DATA".green()
                } else {
                    "⏳ WAITING FOR CONNECTIONS".yellow()
                };
                
                println!("\n{}", overall_status);
                
                // Padding to maintain consistent display
                for _ in 0..2 {
                    println!("{}", " ".repeat(60));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n\nMonitor stopped.");
                break;
            }
        }
    }
    
    Ok(())
}