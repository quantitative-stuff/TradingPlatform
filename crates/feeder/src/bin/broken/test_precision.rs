use feeder::core::{scale_price, unscale_price, get_price_decimals, PRECISION_MANAGER};

fn main() {
    println!("🔍 Exchange-Specific Precision Demonstration\n");
    println!("Same asset, different exchanges, different precision!\n");
    println!("{}", "=".repeat(70));
    
    // Test BTC/USDT across different exchanges
    let btc_price = 50123.456789;
    println!("\n📊 BTC/USDT Price: ${:.6}", btc_price);
    println!("{}", "-".repeat(70));
    
    let exchanges = ["Binance", "Upbit", "OKX", "Bybit"];
    
    for exchange in &exchanges {
        let decimals = get_price_decimals("BTC/USDT", Some(exchange));
        let scaled = scale_price("BTC/USDT", btc_price, Some(exchange));
        let unscaled = unscale_price("BTC/USDT", scaled, Some(exchange));
        
        println!("{:10} | Decimals: {} | Scaled: {:15} | Unscaled: ${:.8}", 
            exchange, decimals, scaled, unscaled);
    }
    
    // Test SHIB/USDT - extreme precision differences
    println!("\n📊 SHIB/USDT Price: 0.000012345678");
    println!("{}", "-".repeat(70));
    
    let shib_price = 0.000012345678;
    
    for exchange in &exchanges {
        let decimals = get_price_decimals("SHIB/USDT", Some(exchange));
        let scaled = scale_price("SHIB/USDT", shib_price, Some(exchange));
        let unscaled = unscale_price("SHIB/USDT", scaled, Some(exchange));
        
        println!("{:10} | Decimals: {:2} | Scaled: {:15} | Unscaled: {:.12}", 
            exchange, decimals, scaled, unscaled);
    }
    
    // Test KRW pairs - different quote currency
    println!("\n📊 BTC/KRW vs BTC/USDT on Upbit");
    println!("{}", "-".repeat(70));
    
    let btc_krw_price = 65000000.0; // 65 million KRW
    let btc_usdt_price = 50000.1234;
    
    let krw_decimals = get_price_decimals("BTC/KRW", Some("Upbit"));
    let usdt_decimals = get_price_decimals("BTC/USDT", Some("Upbit"));
    
    println!("BTC/KRW  | Decimals: {} | Price: ₩{:.0}", krw_decimals, btc_krw_price);
    println!("BTC/USDT | Decimals: {} | Price: ${:.4}", usdt_decimals, btc_usdt_price);
    
    // Show precision loss example
    println!("\n⚠️ Precision Loss Example");
    println!("{}", "-".repeat(70));
    
    let precise_price = 0.0000123456789123456;
    println!("Original SHIB price: {:.16}", precise_price);
    
    for exchange in &exchanges {
        let decimals = get_price_decimals("SHIB/USDT", Some(exchange));
        let scaled = scale_price("SHIB/USDT", precise_price, Some(exchange));
        let unscaled = unscale_price("SHIB/USDT", scaled, Some(exchange));
        let loss = (precise_price - unscaled).abs();
        
        println!("{:10} | {} decimals | Recovered: {:.16} | Loss: {:.2e}", 
            exchange, decimals, unscaled, loss);
    }
    
    // Load custom precision from file example
    println!("\n📁 Loading custom precision from config file...");
    
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        if let Err(e) = PRECISION_MANAGER.load_from_file("config/crypto/crypto_precision.json").await {
            println!("Note: Could not load config file: {}", e);
        } else {
            println!("✅ Successfully loaded custom precision configuration");
        }
    });
    
    println!("\n💡 Key Insights:");
    println!("1. Binance BTC/USDT: 2 decimals (min tick: $0.01)");
    println!("2. OKX BTC/USDT: 1 decimal (min tick: $0.10)");
    println!("3. Upbit BTC/USDT: 4 decimals (min tick: $0.0001)");
    println!("4. SHIB precision varies from 8-10 decimals across exchanges");
    println!("5. KRW pairs typically have 0 decimals (no fractional KRW)");
    println!("\n⚡ This means the SAME order might be:");
    println!("   - Valid on Binance (2 decimals)");
    println!("   - Invalid on OKX (1 decimal)");
    println!("   - More precise on Upbit (4 decimals)");
}