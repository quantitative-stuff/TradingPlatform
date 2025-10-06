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
    println!("🔍 Test OKX Subscription with Proper Symbol Mapping");

    // Load the exact same config as the feeder
    let config_path = "config/crypto/okx_config_full.json";
    let config_str = std::fs::read_to_string(config_path)?;
    let config: ExchangeConfig = serde_json::from_str(&config_str)?;

    println!("📋 Loaded OKX config:");
    println!("   Exchange: {}", config.feed_config.exchange);
    println!("   Asset types: {:?}", config.feed_config.asset_type);
    let spot_symbols = config.subscribe_data.spot_symbols.clone().unwrap_or_default();
    println!("   Spot Symbols: {} total", spot_symbols.len());
    println!("   Stream types: {:?}", config.subscribe_data.stream_type);
    println!("   Order depth: {}", config.subscribe_data.order_depth);

    // Create proper symbol mapper with mappings
    let mut symbol_mapper = SymbolMapper::new();

    // Add OKX mappings for our test symbols (note: OKX uses different format)
    symbol_mapper.add_mapping("OKX", "BTC-USDT", "BTC-USDT");
    symbol_mapper.add_mapping("OKX", "ETH-USDT", "ETH-USDT");
    symbol_mapper.add_mapping("OKX", "SOL-USDT", "SOL-USDT");
    symbol_mapper.add_mapping("OKX", "XRP-USDT", "XRP-USDT");
    symbol_mapper.add_mapping("OKX", "ADA-USDT", "ADA-USDT");

    let symbol_mapper = Arc::new(symbol_mapper);
    println!("✅ Symbol mapper created with mappings for: BTC-USDT, ETH-USDT, SOL-USDT, XRP-USDT, ADA-USDT");

    // Test subscription for spot symbols only (first chunk)
    let asset_type = "spot";
    let ws_url = "wss://ws.okx.com:8443/ws/v5/public";

    println!("\n🔗 Connecting to OKX {} at {}", asset_type, ws_url);
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

    // Create subscription args for OKX format (different from Bybit)
    let mut args = Vec::new();
    for symbol in chunk_symbols {
        // Add trade subscription
        args.push(json!({
            "channel": "trades",
            "instId": symbol
        }));

        // Add orderbook subscription
        args.push(json!({
            "channel": "books",
            "instId": symbol
        }));
    }

    let subscribe_msg = json!({
        "op": "subscribe",
        "args": args
    });

    println!("\n📤 Sending subscription message:");
    println!("   Args ({}): {:?}", args.len(), args);
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
                // Emulate the feeder's process_okx_message function
                if !text.contains("pong") && !text.contains("ping") {
                    let value: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(e) => {
                            println!("❌ JSON parse error: {}", e);
                            continue;
                        }
                    };

                    // Handle subscription responses
                    if let Some(event) = value.get("event") {
                        if event == "subscribe" {
                            subscription_responses += 1;
                            if let Some(msg) = value.get("msg").and_then(Value::as_str) {
                                if msg.is_empty() {
                                    println!("✅ Subscription successful!");
                                } else {
                                    println!("❌ Subscription failed: {}", text);
                                }
                            }
                            continue;
                        }
                    }

                    // Handle market data
                    if let Some(arg) = value.get("arg") {
                        let channel = arg.get("channel").and_then(Value::as_str).unwrap_or("");
                        let inst_id = arg.get("instId").and_then(Value::as_str).unwrap_or("");

                        if channel == "trades" {
                            trade_count += 1;
                            let data = &value["data"];

                            // Process trades
                            if data.is_array() {
                                for trade in data.as_array().unwrap() {
                                    if trade_count <= 3 {
                                        println!("💰 Trade: {} - {} @ {} (qty: {})",
                                            inst_id,
                                            trade.get("px").and_then(Value::as_str).unwrap_or("?"),
                                            trade.get("sz").and_then(Value::as_str).unwrap_or("?"),
                                            trade.get("side").and_then(Value::as_str).unwrap_or("?")
                                        );
                                    }

                                    // Check symbol mapping
                                    match symbol_mapper.map("OKX", inst_id) {
                                        Some(mapped) => {
                                            mapped_trades += 1;
                                            if mapped_trades <= 3 {
                                                println!("✅ Trade mapped: {} -> {} (would send UDP)", inst_id, mapped);
                                            }
                                        },
                                        None => {
                                            unmapped_symbols.insert(inst_id.to_string());
                                        }
                                    }
                                }
                            }
                        } else if channel == "books" {
                            orderbook_count += 1;
                            let data = &value["data"];

                            if orderbook_count <= 3 {
                                if data.is_array() && !data.as_array().unwrap().is_empty() {
                                    let book = &data.as_array().unwrap()[0];
                                    let bids_len = book.get("bids").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                    let asks_len = book.get("asks").and_then(Value::as_array).map(|v| v.len()).unwrap_or(0);
                                    println!("📊 OrderBook: {} (bids: {}, asks: {})",
                                        inst_id, bids_len, asks_len);
                                }
                            }

                            // Check symbol mapping
                            match symbol_mapper.map("OKX", inst_id) {
                                Some(mapped) => {
                                    mapped_orderbooks += 1;
                                    if mapped_orderbooks <= 3 {
                                        println!("✅ OrderBook mapped: {} -> {} (would send UDP)", inst_id, mapped);
                                    }
                                },
                                None => {
                                    unmapped_symbols.insert(inst_id.to_string());
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
    println!("📊 OKX TEST RESULTS:");
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
        println!("✅ SUCCESS! OKX subscriptions and symbol mapping work!");
        println!("📡 Would have sent {} trade UDP packets", mapped_trades);
        println!("📊 Would have sent {} orderbook UDP packets", mapped_orderbooks);
    }

    Ok(())
}