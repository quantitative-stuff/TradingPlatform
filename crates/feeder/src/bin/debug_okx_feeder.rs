use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use feeder::load_config::ExchangeConfig;
use feeder::core::SymbolMapper;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 Debug OKX Feeder - Testing WebSocket connection and subscription");

    // Load the OKX config
    let config_path = "config/crypto/okx_config.json";
    let config_str = std::fs::read_to_string(config_path)?;
    let config: ExchangeConfig = serde_json::from_str(&config_str)?;

    println!("📋 Loaded OKX config:");
    println!("   Exchange: {}", config.feed_config.exchange);
    println!("   Asset types: {:?}", config.feed_config.asset_type);
    let symbols = &config.subscribe_data.codes;
    println!("   Symbols: {} total", symbols.len());
    println!("   Stream types: {:?}", config.subscribe_data.stream_type);
    println!("   Order depth: {}", config.subscribe_data.order_depth);
    println!("   WebSocket URL: {}", config.connect_config.ws_url);

    // Create symbol mapper (same as feeder)
    let symbol_mapper = Arc::new(SymbolMapper::new());

    // Test subscription for first few symbols
    let ws_url = &config.connect_config.ws_url;

    println!("\n🔗 Connecting to OKX at {}", ws_url);
    let (mut ws_stream, _) = connect_async(ws_url).await?;
    println!("✅ Connected successfully!");

    // Test different subscription formats to see what works
    let test_symbols = &symbols[..3.min(symbols.len())]; // First 3 symbols

    println!("\n📝 Testing subscription for {} symbols:", test_symbols.len());
    for symbol in test_symbols {
        println!("   - {}", symbol);
    }

    // Try OKX format subscription (based on their docs)
    let mut subscription_args = Vec::new();

    // Add orderbook subscriptions
    for symbol in test_symbols {
        subscription_args.push(json!({
            "channel": "books",
            "instId": symbol
        }));
    }

    // Add trade subscriptions
    for symbol in test_symbols {
        subscription_args.push(json!({
            "channel": "trades",
            "instId": symbol
        }));
    }

    let subscribe_msg = json!({
        "op": "subscribe",
        "args": subscription_args
    });

    println!("\n📤 Sending OKX subscription message:");
    println!("   Args count: {}", subscription_args.len());
    println!("   Message: {}", serde_json::to_string_pretty(&subscribe_msg)?);

    // Send subscription
    let msg_text = serde_json::to_string(&subscribe_msg)?;
    ws_stream.send(Message::Text(msg_text.into())).await?;

    println!("\n⏰ Listening for messages for 10 seconds...");

    let mut message_count = 0;
    let start_time = std::time::Instant::now();
    let timeout_duration = std::time::Duration::from_secs(10);

    while start_time.elapsed() < timeout_duration {
        tokio::select! {
            msg = ws_stream.next() => {
                if let Some(Ok(Message::Text(text))) = msg {
                    message_count += 1;

                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        if let Some(event) = value.get("event").and_then(Value::as_str) {
                            if event == "error" {
                                println!("❌ Subscription error: {}", text);
                                continue;
                            } else if event == "subscribe" {
                                println!("✅ Subscription successful for: {}", text);
                                continue;
                            }
                        }

                        // Parse data messages
                        if let Some(arg) = value.get("arg") {
                            let channel = arg.get("channel").and_then(Value::as_str).unwrap_or("unknown");
                            let inst_id = arg.get("instId").and_then(Value::as_str).unwrap_or("unknown");

                            match channel {
                                "books" => {
                                    if let Some(data) = value.get("data").and_then(Value::as_array) {
                                        if let Some(book_data) = data.first() {
                                            let asks_count = book_data.get("asks").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
                                            let bids_count = book_data.get("bids").and_then(Value::as_array).map(|b| b.len()).unwrap_or(0);
                                            println!("📚 OrderBook {}: {} bids, {} asks", inst_id, bids_count, asks_count);
                                        }
                                    }
                                },
                                "trades" => {
                                    if let Some(data) = value.get("data").and_then(Value::as_array) {
                                        for trade in data {
                                            let price = trade.get("px").and_then(Value::as_str).unwrap_or("0");
                                            let size = trade.get("sz").and_then(Value::as_str).unwrap_or("0");
                                            let side = trade.get("side").and_then(Value::as_str).unwrap_or("unknown");
                                            println!("💰 Trade {}: {} @ {} ({})", inst_id, size, price, side);
                                        }
                                    }
                                },
                                _ => {
                                    println!("📊 {} message for {}", channel, inst_id);
                                }
                            }
                        }
                    } else {
                        println!("📄 Raw message ({}): {}", message_count,
                                if text.len() > 200 { &text[..200] } else { &text });
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                // Continue listening
            }
        }
    }

    println!("\n📊 Session summary:");
    println!("   Total messages received: {}", message_count);
    println!("   Duration: 10 seconds");

    if message_count == 0 {
        println!("⚠️  No messages received - check subscription format or network");
    } else {
        println!("✅ Data streaming successfully!");
    }

    Ok(())
}