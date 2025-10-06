use feeder::load_config::ExchangeConfig;

fn main() {
    println!("Testing configuration rate limit settings...\n");
    
    // Test Bybit config
    match ExchangeConfig::new("config/crypto/bybit_config_full.json") {
        Ok(config) => {
            println!("Bybit config:");
            println!("  connection_delay_ms: {}", config.connect_config.connection_delay_ms);
            println!("  max_connections: {}", config.connect_config.max_connections);
            println!("  max_retry_delay_secs: {}", config.connect_config.max_retry_delay_secs);
            println!("  initial_retry_delay_secs: {}\n", config.connect_config.initial_retry_delay_secs);
        }
        Err(e) => println!("Error loading Bybit config: {}", e),
    }
    
    // Test Binance config
    match ExchangeConfig::new("config/crypto/binance_config_full.json") {
        Ok(config) => {
            println!("Binance config:");
            println!("  connection_delay_ms: {}", config.connect_config.connection_delay_ms);
            println!("  max_connections: {}", config.connect_config.max_connections);
            println!("  max_retry_delay_secs: {}", config.connect_config.max_retry_delay_secs);
            println!("  initial_retry_delay_secs: {}\n", config.connect_config.initial_retry_delay_secs);
        }
        Err(e) => println!("Error loading Binance config: {}", e),
    }
    
    // Test Upbit config
    match ExchangeConfig::new("config/crypto/upbit_config_full.json") {
        Ok(config) => {
            println!("Upbit config:");
            println!("  connection_delay_ms: {}", config.connect_config.connection_delay_ms);
            println!("  max_connections: {}", config.connect_config.max_connections);
            println!("  max_retry_delay_secs: {}", config.connect_config.max_retry_delay_secs);
            println!("  initial_retry_delay_secs: {}", config.connect_config.initial_retry_delay_secs);
        }
        Err(e) => println!("Error loading Upbit config: {}", e),
    }
}