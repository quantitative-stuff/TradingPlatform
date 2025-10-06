// Debug tool to check CONNECTION_STATS contents
use feeder::core::CONNECTION_STATS;
use colored::*;

#[tokio::main]
async fn main() {
    println!("{}", "🔍 CONNECTION_STATS DEBUG".bright_cyan().bold());
    println!("{}", "═".repeat(60));
    
    loop {
        let stats = CONNECTION_STATS.read();
        
        if stats.is_empty() {
            println!("CONNECTION_STATS is empty - no data recorded");
        } else {
            println!("\nCurrent CONNECTION_STATS contents:");
            println!("{}", "─".repeat(60));
            
            for (key, value) in stats.iter() {
                println!("Key: {}", key.bright_yellow());
                println!("  Connected: {}", value.connected);
                println!("  Disconnected: {}", value.disconnected);
                println!("  Total Connections: {}", value.total_connections);
                println!("  Reconnect Count: {}", value.reconnect_count);
                if let Some(err) = &value.last_error {
                    println!("  Last Error: {}", err.red());
                }
                println!();
            }
        }
        
        println!("\nPress Ctrl+C to exit, waiting 5 seconds...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        
        // Clear screen for next update
        print!("\x1B[2J\x1B[1;1H");
    }
}