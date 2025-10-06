// Detailed monitor showing connection counts and data flow
use anyhow::Result;
use std::time::Duration;
use tokio::time::interval;
use colored::*;
use feeder::core::{CONNECTION_STATS, TRADES, ORDERBOOKS};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "📊 DETAILED WEBSOCKET MONITOR".bright_cyan().bold());
    println!("{}", "═".repeat(80));
    println!("Shows connections by exchange and asset type");
    println!("Press Ctrl+C to exit\n");
    
    let mut check_interval = interval(Duration::from_secs(3));
    let mut iteration = 0;
    
    loop {
        tokio::select! {
            _ = check_interval.tick() => {
                iteration += 1;
                
                // Clear screen and move to top
                print!("\x1B[2J\x1B[1;1H");
                
                println!("{}", "📊 DETAILED WEBSOCKET MONITOR".bright_cyan().bold());
                println!("Update #{} - {}", iteration, chrono::Local::now().format("%H:%M:%S"));
                println!("{}", "═".repeat(80));
                
                // Connection stats
                let conn_stats = CONNECTION_STATS.read();
                
                println!("\n{}", "EXCHANGE CONNECTIONS (5 symbols per connection):".bright_white().bold());
                println!("{}", "─".repeat(80));
                println!("{:<25} {:>12} {:>12} {:>10} {:>12}", 
                    "Exchange/Type", "Connected", "Disconnected", "Reconnects", "Status");
                println!("{}", "─".repeat(80));
                
                // Group by exchange for totals
                let mut exchange_totals: HashMap<String, (usize, usize, usize)> = HashMap::new();
                let mut detailed_stats = Vec::new();
                
                for (key, stats) in conn_stats.iter() {
                    // Store detailed stats
                    detailed_stats.push((key.clone(), stats.clone()));
                    
                    // Extract exchange name (before underscore)
                    let exchange = key.split('_').next().unwrap_or(key);
                    let entry = exchange_totals.entry(exchange.to_string())
                        .or_insert((0, 0, 0));
                    entry.0 += stats.connected;
                    entry.1 += stats.disconnected;
                    entry.2 += stats.reconnect_count;
                }
                
                // Sort by exchange name
                let mut sorted_stats = detailed_stats;
                sorted_stats.sort_by(|a, b| a.0.cmp(&b.0));
                
                for (key, stats) in sorted_stats.iter() {
                    let status = if stats.connected == 0 {
                        "❌ OFFLINE".red()
                    } else if stats.disconnected > 0 {
                        format!("⚠️  PARTIAL").yellow()
                    } else {
                        "✅ ACTIVE".green()
                    };
                    
                    // Estimate symbols (5 per connection)
                    let estimated_symbols = stats.connected * 5;
                    
                    println!("{:<25} {:>12} {:>12} {:>10} {:>12}  (~{} symbols)", 
                        key,
                        if stats.connected > 0 { 
                            stats.connected.to_string().green() 
                        } else { 
                            stats.connected.to_string().red() 
                        },
                        if stats.disconnected > 0 {
                            stats.disconnected.to_string().yellow()
                        } else {
                            stats.disconnected.to_string().bright_black()
                        },
                        stats.reconnect_count,
                        status,
                        estimated_symbols
                    );
                }
                
                if !exchange_totals.is_empty() {
                    println!("{}", "─".repeat(80));
                    println!("\n{}", "EXCHANGE TOTALS:".bright_white().bold());
                    println!("{}", "─".repeat(80));
                    println!("{:<15} {:>12} {:>12} {:>10}", 
                        "Exchange", "Connected", "Disconnected", "Reconnects");
                    println!("{}", "─".repeat(80));
                    
                    let mut total_connected = 0;
                    let mut total_disconnected = 0;
                    
                    for (exchange, (connected, disconnected, reconnects)) in exchange_totals.iter() {
                        println!("{:<15} {:>12} {:>12} {:>10}", 
                            exchange,
                            if *connected > 0 {
                                connected.to_string().green()
                            } else {
                                connected.to_string().red()
                            },
                            if *disconnected > 0 {
                                disconnected.to_string().yellow()
                            } else {
                                disconnected.to_string().bright_black()
                            },
                            reconnects
                        );
                        total_connected += connected;
                        total_disconnected += disconnected;
                    }
                    
                    println!("{}", "─".repeat(80));
                    println!("{:<15} {:>12} {:>12}", 
                        "TOTAL",
                        total_connected.to_string().bright_white().bold(),
                        total_disconnected.to_string().yellow()
                    );
                    
                    // Estimate total symbols
                    let total_symbols = total_connected * 5;
                    println!("\nEstimated total symbols being monitored: {}", 
                        total_symbols.to_string().bright_cyan().bold());
                }
                
                // Data stats
                let trades = TRADES.read();
                let orderbooks = ORDERBOOKS.read();
                
                println!("\n{}", "DATA FLOW:".bright_white().bold());
                println!("{}", "─".repeat(40));
                println!("  Trades in buffer     : {}", 
                    trades.len().to_string().cyan());
                println!("  Orderbooks in buffer : {}", 
                    orderbooks.len().to_string().cyan());
                
                // Calculate rates
                let rate = if iteration > 1 {
                    (trades.len() + orderbooks.len()) as f64 / (iteration as f64 * 3.0)
                } else {
                    0.0
                };
                println!("  Avg rate             : {:.1} msg/sec", rate);
                
                // Overall status
                println!("\n{}", "SYSTEM STATUS:".bright_white().bold());
                let total_connected = exchange_totals.values()
                    .map(|(c, _, _)| c)
                    .sum::<usize>();
                
                let overall_status = if total_connected == 0 {
                    "⏳ NO CONNECTIONS - Check if feeder is running".red()
                } else {
                    format!("✅ {} WebSocket connections active", total_connected).green()
                };
                println!("{}", overall_status);
                
                println!("\n{}", "─".repeat(80));
                println!("Tips: Each connection handles 5 symbols (both trade and orderbook)");
                println!("      Connection format: Exchange_AssetType (e.g., Binance_spot)");
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n\nMonitor stopped.");
                break;
            }
        }
    }
    
    Ok(())
}