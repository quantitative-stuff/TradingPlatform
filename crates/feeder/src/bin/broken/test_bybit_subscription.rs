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
    println!("🔍 Test Bybit Subscription with Proper Symbol Mapping");

    // Load the exact same config as the feeder
    let config_path = "config/crypto/bybit_config_full.json";
    let config_str = std::fs::read_to_string(config_path)?;
    let config: ExchangeConfig = serde_json::from_str(&config_str)?;

    // Get spot symbols from the config
    let spot_symbols = config.subscribe_data.spot_symbols.clone().unwrap_or_default();
    
    println!("📋 Loaded Bybit config:");
    println!("   Exchange: {}", config.feed_config.exchange);
    println!("   Asset types: {:?}", config.feed_config.asset_type);
    println!("   Spot Symbols: {} total", spot_symbols.len());
    println!("   Stream types: {:?}", config.subscribe_data.stream_type);
    println!("   Order depth: {}", config.subscribe_data.order_depth);

    // Create proper symbol mapper with mappings
    let mut symbol_mapper = SymbolMapper::new();

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
    let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, spot_symbols.len());
    let chunk_symbols = &spot_symbols[start_idx..end_idx];

    println!("\n📝 Subscribing to chunk {} ({} symbols):", chunk_idx, chunk_symbols.len());
    for symbol in chunk_symbols {
        println!("   - {}", symbol);
    }

    // Add symbol mappings for all symbols in the chunk
    for symbol in chunk_symbols {
        // Map BTCUSDT -> BTC-USDT format
        if symbol.ends_with("USDT") {
            let base = symbol.replace("USDT", "");
            let mapped = format!("{}-USDT", base);
            symbol_mapper.add_mapping("Bybit", symbol, &mapped);
        }
    }
    
    let symbol_mapper = Arc::new(symbol_mapper);
    println!("✅ Symbol mapper created with mappings for chunk symbols");
    
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
    
    println!("\n⏰ Listening for messages for 10 seconds...");

    let mut subscription_responses = 0;
    let mut trade_count = 0;
    let mut orderbook_count = 0;
    let mut mapped_trades = 0;
    let mut mapped_orderbooks = 0;
    let mut unmapped_symbols = std::collections::HashSet::new();
    let start_time = std::time::Instant::now();

    while start_time.elapsed() < Duration::from_secs(10) {
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
                                        Some(mapped) => {
                                            mapped_trades += 1;
                                            if mapped_trades <= 3 {
                                                println!("✅ Trade mapped: {} -> {} (would send UDP)", original_symbol, mapped);
                                            }
                                        },
                                        None => {
                                            unmapped_symbols.insert(original_symbol.to_string());
                                        }
                                    }
                                }
                            }
                        } else if topic.starts_with("orderbook") {
                            orderbook_count += 1;
                            let data = &value["data"];
                            let original_symbol = data.get("s").and_then(Value::as_str).unwrap_or_default();

                            if orderbook_count <= 3 {
                                let bids_len = data.get("b").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                let asks_len = data.get("a").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                println!("📊 OrderBook: {} (bids: {}, asks: {})",
                                    original_symbol, bids_len, asks_len);
                            }

                            // Check symbol mapping (same as feeder)
                            match symbol_mapper.map("Bybit", original_symbol) {
                                Some(mapped) => {
                                    mapped_orderbooks += 1;
                                    if mapped_orderbooks <= 3 {
                                        println!("✅ OrderBook mapped: {} -> {} (would send UDP)", original_symbol, mapped);
                                    }
                                },
                                None => {
                                    unmapped_symbols.insert(original_symbol.to_string());
                                }
                            }
                        }
                    }
                }
            }
            Ok(Some(Ok(_other))) => {
                // Other message types (ping/pong/etc)
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
    println!("📊 TEST RESULTS:");
    println!("⚙️  Subscription responses: {}", subscription_responses);
    println!("💰 Trade messages: {}", trade_count);
    println!("📊 OrderBook messages: {}", orderbook_count);
    println!("✅ Mapped trades: {}", mapped_trades);
    println!("✅ Mapped orderbooks: {}", mapped_orderbooks);
    println!("❌ Unmapped symbols: {}", unmapped_symbols.len());

    if !unmapped_symbols.is_empty() {
        println!("\n❌ Unmapped symbols found:");
        for symbol in &unmapped_symbols {
            println!("   - {}", symbol);
        }
    }

    println!("\n🔍 DIAGNOSIS:");
    if subscription_responses == 0 {
        println!("❌ No subscription responses received - subscription message failed");
    } else if mapped_trades == 0 && mapped_orderbooks == 0 {
        println!("❌ No mapped data - symbol mapping problem");
    } else {
        println!("✅ SUCCESS! Symbol mapping works, feeder should work with proper symbol mapper initialization!");
        println!("📡 Would have sent {} trade UDP packets", mapped_trades);
        println!("📊 Would have sent {} orderbook UDP packets", mapped_orderbooks);
    }

    Ok(())
}