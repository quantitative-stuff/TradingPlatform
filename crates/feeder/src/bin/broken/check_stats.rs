// Check what's actually in CONNECTION_STATS
use feeder::core::CONNECTION_STATS;
use colored::*;

#[tokio::main]
async fn main() {
    println!("{}", "🔍 CHECKING CONNECTION_STATS".bright_cyan().bold());
    println!("This will show what's actually stored in CONNECTION_STATS");
    println!("Make sure feeder is running with Binance selected!\n");
    
    loop {
        let stats = CONNECTION_STATS.read();
        
        println!("{}", "─".repeat(60));
        println!("Time: {}", chrono::Local::now().format("%H:%M:%S"));
        println!("Total entries in CONNECTION_STATS: {}", stats.len());
        
        if stats.is_empty() {
            println!("{}", "❌ CONNECTION_STATS is EMPTY!".red());
            println!("Possible issues:");
            println!("  1. Feeder is not running");
            println!("  2. Binance connection code is not being reached");
            println!("  3. CONNECTION_STATS update is failing");
        } else {
            println!("\n{}", "Entries found:".green());
            for (key, value) in stats.iter() {
                println!("\n  Key: {}", key.bright_yellow());
                println!("    Connected: {}", value.connected);
                println!("    Disconnected: {}", value.disconnected);
                println!("    Total Connections: {}", value.total_connections);
                println!("    Reconnect Count: {}", value.reconnect_count);
            }
        }
        
        println!("\nWaiting 3 seconds... (Press Ctrl+C to exit)");
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}