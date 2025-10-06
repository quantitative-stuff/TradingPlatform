use feeder::core::{TradeData, OrderBookData};
use serde_json;

fn main() {
    // Create sample trade
    let trade = TradeData {
        exchange: "Binance".to_string(),
        symbol: "BTC-USDT".to_string(),
        asset_type: "spot".to_string(),
        price: 43250.5,
        quantity: 0.012,
        timestamp: 1698765432123,
    };
    
    // Measure JSON size
    let json = serde_json::to_string(&trade).unwrap();
    let packet = format!("TRADE|{}", json);
    
    println!("Trade JSON: {}", json);
    println!("Trade packet size: {} bytes", packet.len());
    println!("Trade packet: {}", packet);
    
    // Create sample orderbook
    let mut bids = Vec::new();
    let mut asks = Vec::new();
    
    // Add 20 levels each
    for i in 0..20 {
        bids.push((43250.0 - i as f64 * 0.1, 1.5 + i as f64 * 0.1));
        asks.push((43251.0 + i as f64 * 0.1, 1.2 + i as f64 * 0.1));
    }
    
    let orderbook = OrderBookData {
        exchange: "Bybit".to_string(),
        symbol: "ETH-USDT".to_string(),
        asset_type: "linear".to_string(),
        bids,
        asks,
        timestamp: 1698765432123,
    };
    
    let ob_json = serde_json::to_string(&orderbook).unwrap();
    let ob_packet = format!("ORDERBOOK|{}", ob_json);
    
    println!("\nOrderbook packet size: {} bytes", ob_packet.len());
    println!("First 200 chars: {}...", &ob_packet[..200.min(ob_packet.len())]);
    
    // Calculate overhead
    println!("\n=== JSON Overhead Analysis ===");
    println!("Trade raw data size: ~31 bytes (if binary)");
    println!("Trade JSON size: {} bytes", json.len());
    println!("JSON overhead: {}x larger", json.len() / 31);
    
    // Stats packet comparison
    let stats_packet = format!("STATS|{}|{}|{}|{}", "Binance", 1523, 892, 1698765432);
    println!("\nStats packet (pipe-delimited): {} bytes", stats_packet.len());
    println!("Stats packet: {}", stats_packet);
}