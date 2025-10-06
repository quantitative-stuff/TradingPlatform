use std::io::{self, Write};

fn wait_for_enter() {
    println!("\nPress Enter to exit...");
    let _ = io::stdout().flush();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap_or_default();
}

fn main() {
    // Set panic hook first
    std::panic::set_hook(Box::new(|info| {
        eprintln!("\n=== PANIC ===");
        eprintln!("{}", info);
        eprintln!("=============");
        wait_for_enter();
    }));

    println!("Test feeder starting...");
    println!("Step 1: Basic setup");
    
    // Try to load config
    let config_path = "config/crypto/binance_config_full.json";
    println!("Step 2: Loading config from {}", config_path);
    
    match std::fs::read_to_string(config_path) {
        Ok(content) => {
            println!("Step 3: Config loaded, size: {} bytes", content.len());
            
            // Try to parse JSON
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => {
                    println!("Step 4: JSON parsed successfully");
                    if let Some(symbols) = json["subscribe_data"]["spot_symbols"].as_array() {
                        println!("Step 5: Found {} spot symbols", symbols.len());
                    }
                },
                Err(e) => {
                    eprintln!("ERROR parsing JSON: {}", e);
                }
            }
        },
        Err(e) => {
            eprintln!("ERROR loading config: {}", e);
        }
    }
    
    println!("\nTest completed!");
    wait_for_enter();
}