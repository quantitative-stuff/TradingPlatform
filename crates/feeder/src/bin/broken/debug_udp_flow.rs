use std::sync::Arc;
use feeder::core::{
    SymbolMapper, TradeData, OrderBookData, 
    init_global_optimized_udp_sender, get_optimized_udp_sender
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 Debug UDP Flow - Testing complete Bybit to UDP pipeline");
    
    // Step 1: Initialize UDP sender (same as feeder)
    println!("📡 Step 1: Initializing UDP sender...");
    const MONITOR_ENDPOINT: &str = "239.1.1.1:9001";
    if let Err(e) = init_global_optimized_udp_sender("0.0.0.0:0", MONITOR_ENDPOINT).await {
        println!("❌ Failed to initialize UDP sender: {}", e);
        return Ok(());
    }
    println!("✅ UDP sender initialized");
    
    // Step 2: Get UDP sender reference
    let udp_sender = get_optimized_udp_sender();
    if udp_sender.is_none() {
        println!("❌ UDP sender not available after initialization");
        return Ok(());
    }
    let udp_sender = udp_sender.unwrap();
    println!("✅ UDP sender reference obtained");
    
    // Step 3: Create symbol mapper (same as feeder)
    println!("🗺️  Step 3: Creating symbol mapper...");
    let symbol_mapper = Arc::new(SymbolMapper::new());
    println!("✅ Symbol mapper created");
    
    // Step 4: Connect to Bybit WebSocket (minimal version)
    println!("🔗 Step 4: Connecting to Bybit WebSocket...");
    let url = "wss://stream.bybit.com/v5/public/spot";
    let (mut ws_stream, _) = connect_async(url).await?;
    println!("✅ Connected to Bybit");
    
    // Step 5: Subscribe to one symbol only for testing
    println!("📝 Step 5: Subscribing to BTCUSDT...");
    let subscribe_msg = json!({
        "op": "subscribe",
        "args": ["publicTrade.BTCUSDT", "orderbook.50.BTCUSDT"]
    });
    
    ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await?;
    println!("✅ Subscription sent");
    
    // Step 6: Process messages and test UDP sending
    println!("⏰ Step 6: Processing messages for 15 seconds...");
    println!("   Will test both symbol mapping and UDP sending...");
    
    let mut trade_count = 0;
    let mut orderbook_count = 0;
    let mut udp_send_attempts = 0;
    let mut udp_send_errors = 0;
    let start_time = std::time::Instant::now();
    
    while start_time.elapsed() < std::time::Duration::from_secs(15) {
        match tokio::time::timeout(
            std::time::Duration::from_secs(1), 
            ws_stream.next()
        ).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    // Skip pong messages
                    if text.contains("pong") { continue; }
                    
                    // Handle subscription confirmations
                    if let Some(op) = value.get("op") {
                        if op == "subscribe" {
                            println!("✅ Subscription confirmed: {}", text);
                            continue;
                        }
                    }
                    
                    // Process market data
                    if let Some(topic) = value.get("topic").and_then(Value::as_str) {
                        println!("📨 Received topic: {}", topic);
                        
                        if topic.starts_with("publicTrade") {
                            // Test trade processing (same as Bybit code)
                            if let Some(data) = value.get("data").and_then(Value::as_array) {
                                for trade_data in data {
                                    let original_symbol = trade_data.get("s").and_then(Value::as_str).unwrap_or_default();
                                    println!("💰 Processing trade for symbol: {}", original_symbol);
                                    
                                    // Test symbol mapping (with our fix)
                                    let common_symbol = match symbol_mapper.map("Bybit", original_symbol) {
                                        Some(symbol) => symbol,
                                        None => {
                                            println!("🗺️  No mapping found, using original symbol: {}", original_symbol);
                                            original_symbol.to_string()
                                        }
                                    };
                                    
                                    // Create TradeData (same structure as Bybit code)
                                    let price = trade_data.get("p")
                                        .and_then(Value::as_str)
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or_default();
                                    let quantity = trade_data.get("v")
                                        .and_then(Value::as_str)
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or_default();
                                    let timestamp = trade_data.get("T")
                                        .and_then(Value::as_i64)
                                        .unwrap_or_default();
                                    
                                    let trade = TradeData {
                                        exchange: "Bybit".to_string(),
                                        symbol: common_symbol.clone(),
                                        asset_type: "spot".to_string(),
                                        price,
                                        quantity,
                                        timestamp,
                                    };
                                    
                                    // Test UDP sending
                                    udp_send_attempts += 1;
                                    match udp_sender.send_trade(trade) {
                                        Ok(_) => {
                                            trade_count += 1;
                                            println!("✅ Trade UDP packet sent successfully for {}", common_symbol);
                                        }
                                        Err(e) => {
                                            udp_send_errors += 1;
                                            println!("❌ Failed to send trade UDP packet: {}", e);
                                        }
                                    }
                                }
                            }
                        } else if topic.starts_with("orderbook") {
                            // Test orderbook processing
                            let data = &value["data"];
                            let original_symbol = data.get("s").and_then(Value::as_str).unwrap_or_default();
                            println!("📊 Processing orderbook for symbol: {}", original_symbol);
                            
                            // Test symbol mapping (with our fix)
                            let common_symbol = match symbol_mapper.map("Bybit", original_symbol) {
                                Some(symbol) => symbol,
                                None => {
                                    println!("🗺️  No mapping found, using original symbol: {}", original_symbol);
                                    original_symbol.to_string()
                                }
                            };
                            
                            // Create simple OrderBookData for testing
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as i64;
                            
                            let orderbook = OrderBookData {
                                exchange: "Bybit".to_string(),
                                symbol: common_symbol.clone(),
                                asset_type: "spot".to_string(),
                                bids: vec![], // Simplified for testing
                                asks: vec![], // Simplified for testing
                                timestamp,
                            };
                            
                            // Test UDP sending
                            udp_send_attempts += 1;
                            match udp_sender.send_orderbook(orderbook) {
                                Ok(_) => {
                                    orderbook_count += 1;
                                    println!("✅ OrderBook UDP packet sent successfully for {}", common_symbol);
                                }
                                Err(e) => {
                                    udp_send_errors += 1;
                                    println!("❌ Failed to send orderbook UDP packet: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            Ok(Some(Ok(other))) => {
                println!("🔔 Other message type: {:?}", other);
            }
            Ok(Some(Err(e))) => {
                println!("❌ WebSocket error: {}", e);
                break;
            }
            Ok(None) => {
                println!("🔚 Connection closed");
                break;
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }
    
    println!("\n========================================");
    println!("📊 FINAL UDP FLOW DEBUG RESULTS:");
    println!("⏱️  Total runtime: {:.1}s", start_time.elapsed().as_secs_f64());
    println!("🔄 UDP send attempts: {}", udp_send_attempts);
    println!("❌ UDP send errors: {}", udp_send_errors);
    println!("✅ UDP sends successful: {}", udp_send_attempts - udp_send_errors);
    println!("💰 Trade packets sent: {}", trade_count);
    println!("📊 OrderBook packets sent: {}", orderbook_count);
    
    println!("\n🔍 DIAGNOSIS:");
    if udp_send_attempts == 0 {
        println!("❌ No market data received from Bybit - subscription issue");
    } else if udp_send_errors == udp_send_attempts {
        println!("❌ All UDP sends failed - UDP sender issue");
    } else if trade_count > 0 || orderbook_count > 0 {
        println!("✅ SUCCESS: UDP packets are being sent!");
        println!("   The issue might be in the UDP receiver/monitor");
    } else {
        println!("⚠️  Partial success - check specific error messages above");
    }
    
    Ok(())
}