use feeder::core::{TRADES, ORDERBOOKS};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() {
    println!("Monitoring Market Data Feed Statistics...\n");
    
    loop {
        // Clear screen (Windows)
        print!("\x1B[2J\x1B[1;1H");
        
        println!("=== Market Data Feed Statistics ===");
        println!("Time: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        println!();
        
        // Analyze trades
        let trades = TRADES.read();
        let mut exchange_trade_counts: HashMap<String, usize> = HashMap::new();
        let mut symbol_counts: HashMap<String, usize> = HashMap::new();
        
        for trade in trades.iter() {
            *exchange_trade_counts.entry(trade.exchange.clone()).or_insert(0) += 1;
            *symbol_counts.entry(trade.symbol.clone()).or_insert(0) += 1;
        }
        
        println!("TRADES:");
        println!("  Total: {} trades in memory", trades.len());
        for (exchange, count) in exchange_trade_counts.iter() {
            println!("  {}: {} trades", exchange, count);
        }
        println!("  Unique symbols: {}", symbol_counts.len());
        
        // Show top 5 most active symbols
        let mut symbol_vec: Vec<_> = symbol_counts.iter().collect();
        symbol_vec.sort_by(|a, b| b.1.cmp(a.1));
        
        if !symbol_vec.is_empty() {
            println!("\n  Top 5 Active Symbols:");
            for (symbol, count) in symbol_vec.iter().take(5) {
                println!("    {} - {} trades", symbol, count);
            }
        }
        
        drop(trades); // Release lock
        
        println!();
        
        // Analyze orderbooks
        let orderbooks = ORDERBOOKS.read();
        let mut exchange_ob_counts: HashMap<String, usize> = HashMap::new();
        let mut ob_symbol_counts: HashMap<String, usize> = HashMap::new();
        
        for ob in orderbooks.iter() {
            *exchange_ob_counts.entry(ob.exchange.clone()).or_insert(0) += 1;
            *ob_symbol_counts.entry(ob.symbol.clone()).or_insert(0) += 1;
        }
        
        println!("ORDERBOOKS:");
        println!("  Total: {} orderbook snapshots", orderbooks.len());
        for (exchange, count) in exchange_ob_counts.iter() {
            println!("  {}: {} snapshots", exchange, count);
        }
        println!("  Unique symbols: {}", ob_symbol_counts.len());
        
        drop(orderbooks); // Release lock
        
        println!("\n[Refreshing every 5 seconds... Press Ctrl+C to exit]");
        
        time::sleep(Duration::from_secs(5)).await;
    }
}