use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Testing OKX Real-Time Data Flow ===\n");

    // Connect to OKX WebSocket
    let url = "wss://ws.okx.com:8443/ws/v5/public";
    let (ws_stream, _) = connect_async(url).await?;
    println!("✅ Connected to OKX WebSocket\n");

    let (mut write, mut read) = ws_stream.split();

    // Subscribe to BTC-USDT trades and orderbook
    let subscribe_msg = json!({
        "op": "subscribe",
        "args": [
            {"channel": "trades", "instId": "BTC-USDT"},
            {"channel": "books5", "instId": "BTC-USDT"}
        ]
    });

    println!("📤 Subscribing to BTC-USDT trades and orderbook...");
    write.send(Message::Text(subscribe_msg.to_string().into())).await?;

    let mut trade_count = 0;
    let mut book_count = 0;
    let mut last_trade_price = 0.0;
    let start = std::time::Instant::now();

    println!("📊 Monitoring data for 15 seconds...\n");

    // Monitor for 15 seconds
    while start.elapsed() < std::time::Duration::from_secs(15) {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(std::time::Duration::from_millis(100), read.next()).await {

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(event) = json.get("event") {
                    println!("📣 Event: {}", event);
                    if event == "error" {
                        println!("   Error message: {}", json.get("msg").unwrap_or(&json!("unknown")));
                    }
                } else if let Some(arg) = json.get("arg") {
                    if let Some(channel) = arg.get("channel").and_then(|v| v.as_str()) {
                        match channel {
                            "trades" => {
                                trade_count += 1;
                                if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
                                    for trade in data {
                                        if let Some(px) = trade.get("px").and_then(|v| v.as_str()) {
                                            if let Ok(price) = px.parse::<f64>() {
                                                last_trade_price = price;
                                                if trade_count == 1 || trade_count % 10 == 0 {
                                                    println!("💰 Trade #{}: BTC-USDT @ ${:.2}", trade_count, price);
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            "books5" => {
                                book_count += 1;
                                if book_count == 1 {
                                    println!("📚 First orderbook received!");
                                    if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
                                        if let Some(book) = data.first() {
                                            if let (Some(bids), Some(asks)) = (
                                                book.get("bids").and_then(|v| v.as_array()),
                                                book.get("asks").and_then(|v| v.as_array())
                                            ) {
                                                if let (Some(best_bid), Some(best_ask)) = (bids.first(), asks.first()) {
                                                    println!("   Best Bid: {}", best_bid[0]);
                                                    println!("   Best Ask: {}", best_ask[0]);
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                } else if text == "pong" {
                    // Handle pong response
                }
            }
        }
    }

    println!("\n=== Test Results ===");
    println!("✅ Trades received: {}", trade_count);
    println!("✅ OrderBooks received: {}", book_count);
    if last_trade_price > 0.0 {
        println!("✅ Last BTC price: ${:.2}", last_trade_price);
    }

    if trade_count > 0 && book_count > 0 {
        println!("\n🎉 SUCCESS: OKX is feeding data correctly!");
    } else {
        println!("\n❌ FAILED: No data received from OKX");
        println!("   Check if BTC-USDT symbol is correct");
    }

    Ok(())
}