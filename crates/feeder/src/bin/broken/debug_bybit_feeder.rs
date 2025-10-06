use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use feeder::core::SymbolMapper;
use feeder::load_config::ExchangeConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 Debug Bybit Feeder - Emulating feeder logic exactly");
    
    // Load the exact same config as the feeder
    let config_path = "config/crypto/bybit_config_full.json";
    let config_str = std::fs::read_to_string(config_path)?;
    let config: ExchangeConfig = serde_json::from_str(&config_str)?;
    
    println!("📋 Loaded Bybit config:");
    println!("   Exchange: {}", config.feed_config.exchange);
    println!("   Asset types: {:?}", config.feed_config.asset_type);
    let symbols = &config.subscribe_data.codes;
    println!("   Symbols: {} total", symbols.len());
    println!("   Stream types: {:?}", config.subscribe_data.stream_type);
    println!("   Order depth: {}", config.subscribe_data.order_depth);
    
    // Create symbol mapper (same as feeder)
    let symbol_mapper = Arc::new(SymbolMapper::new());
    
    // Test subscription for spot symbols only (first chunk)
    let asset_type = "spot";
    let ws_url = "wss://stream.bybit.com/v5/public/spot";
    
    println!("\n🔗 Connecting to Bybit {} at {}", asset_type, ws_url);
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await?;
    println!("✅ Connected successfully!");
    
    // Emulate feeder subscription logic exactly
    const SYMBOLS_PER_CONNECTION: usize = 5;
    let chunk_idx = 0; // First chunk
    let start_idx = chunk_idx * SYMBOLS_PER_CONNECTION;
    let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, symbols.len());
    let chunk_symbols = &symbols[start_idx..end_idx];
    
    println!("\n📝 Subscribing to chunk {} ({} symbols):", chunk_idx, chunk_symbols.len());
    for symbol in chunk_symbols {
        println!("   - {}", symbol);
    }
    
    // Create subscription topics (same as feeder)
    let mut topics = Vec::new();
    for code in chunk_symbols {
        let formatted_symbol = code.clone(); // Same as feeder
        
        // Add both trade and orderbook topics for each symbol (same as feeder)
        topics.push(format!("publicTrade.{}", formatted_symbol));
        topics.push(format!("orderbook.{}.{}", 
            config.subscribe_data.order_depth,
            formatted_symbol
        ));
    }
    
    let subscribe_msg = json!({
        "op": "subscribe",
        "args": topics
    });
    
    println!("\n📤 Sending subscription message:");
    println!("   Topics ({}): {:?}", topics.len(), topics);
    println!("   Message: {}", subscribe_msg);
    
    let message = Message::Text(subscribe_msg.to_string().into());
    ws_stream.send(message).await?;
    
    println!("\n⏰ Listening for messages for 15 seconds...");
    
    let mut subscription_responses = 0;
    let mut trade_count = 0;
    let mut orderbook_count = 0;
    let mut other_messages = 0;
    let mut unique_symbols = std::collections::HashSet::new();
    let start_time = std::time::Instant::now();
    
    while start_time.elapsed() < Duration::from_secs(15) {
        match tokio::time::timeout(Duration::from_secs(1), ws_stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                // Emulate the feeder's process_bybit_message function
                if !text.contains("pong") && !text.contains("ping") {
                    let value: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(e) => {
                            println!("❌ JSON parse error: {}", e);
                            continue;
                        }
                    };
                    
                    // Handle subscription responses (same as feeder)
                    if let Some(op) = value.get("op") {
                        if op == "subscribe" {
                            subscription_responses += 1;
                            if let Some(success) = value.get("success").and_then(|v| v.as_bool()) {
                                if success {
                                    println!("✅ Subscription successful!");
                                } else {
                                    println!("❌ Subscription failed: {}", text);
                                }
                            }
                            continue;
                        }
                    }
                    
                    // Handle market data (same as feeder)
                    if let Some(topic) = value.get("topic").and_then(Value::as_str) {
                        if topic.starts_with("publicTrade") {
                            trade_count += 1;
                            let data = &value["data"];
                            
                            // Process trades (same logic as feeder)
                            if data.is_array() {
                                for trade in data.as_array().unwrap() {
                                    let original_symbol = trade.get("s").and_then(Value::as_str).unwrap_or_default();
                                    unique_symbols.insert(original_symbol.to_string());
                                    
                                    if trade_count <= 3 {
                                        println!("💰 Trade: {} - {} @ {} (qty: {})", 
                                            original_symbol,
                                            trade.get("p").and_then(Value::as_str).unwrap_or("?"),
                                            trade.get("v").and_then(Value::as_str).unwrap_or("?"),
                                            trade.get("S").and_then(Value::as_str).unwrap_or("?")
                                        );
                                    }
                                    
                                    // Check symbol mapping (same as feeder)
                                    match symbol_mapper.map("Bybit", original_symbol) {
                                        Some(_mapped) => {
                                            // Would create TradeData and send UDP here
                                        },
                                        None => {
                                            if trade_count <= 5 {
                                                println!("⚠️  No mapping found for trade symbol: {}", original_symbol);
                                            }
                                        }
                                    }
                                }
                            }
                        } else if topic.starts_with("orderbook") {
                            orderbook_count += 1;
                            let data = &value["data"];
                            let original_symbol = data.get("s").and_then(Value::as_str).unwrap_or_default();
                            unique_symbols.insert(original_symbol.to_string());
                            
                            if orderbook_count <= 3 {
                                let bids_len = data.get("b").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                let asks_len = data.get("a").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                println!("📊 OrderBook: {} (bids: {}, asks: {})", 
                                    original_symbol, bids_len, asks_len);
                            }
                            
                            // Check symbol mapping (same as feeder)
                            match symbol_mapper.map("Bybit", original_symbol) {
                                Some(_mapped) => {
                                    // Would create OrderBookData and send UDP here
                                },
                                None => {
                                    if orderbook_count <= 5 {
                                        println!("⚠️  No mapping found for orderbook symbol: {}", original_symbol);
                                    }
                                }
                            }
                        } else {
                            other_messages += 1;
                            if other_messages <= 3 {
                                println!("📡 Other topic: {}", topic);
                            }
                        }
                    }
                }
            }
            Ok(Some(Ok(other))) => {
                println!("🔔 Non-text message: {:?}", other);
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
    println!("📊 DEBUG RESULTS:");
    println!("⚙️  Subscription responses: {}", subscription_responses);
    println!("💰 Trade messages: {}", trade_count);
    println!("📊 OrderBook messages: {}", orderbook_count);
    println!("📡 Other messages: {}", other_messages);
    println!("🎯 Unique symbols seen: {}", unique_symbols.len());
    
    if unique_symbols.len() > 0 {
        println!("\n🎯 Symbols received:");
        for symbol in &unique_symbols {
            println!("   - {}", symbol);
        }
    }
    
    println!("\n🔍 DIAGNOSIS:");
    if subscription_responses == 0 {
        println!("❌ No subscription responses received - subscription message failed");
    } else if trade_count == 0 && orderbook_count == 0 {
        println!("⚠️  Subscriptions confirmed but no market data - symbol issues?");
    } else {
        println!("✅ Feeder logic should work - issue may be in symbol mapping or UDP sending");
        
        // Test symbol mapping for each received symbol
        println!("\n🗺️  Testing symbol mapping:");
        for symbol in &unique_symbols {
            match symbol_mapper.map("Bybit", symbol) {
                Some(mapped) => println!("   {} -> {} ✅", symbol, mapped),
                None => println!("   {} -> NO MAPPING ❌", symbol),
            }
        }
    }
    
    Ok(())
}