use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Testing OKX WebSocket with minimal setup...\n");

    let ws_url = "wss://ws.okx.com:8443/ws/v5/public";
    let (mut ws_stream, _) = connect_async(ws_url).await?;
    println!("✅ Connected to OKX\n");

    // Subscribe to just BTC-USDT trades to test
    let subscribe_msg = json!({
        "op": "subscribe",
        "args": [{
            "channel": "trades",
            "instId": "BTC-USDT"
        }]
    });

    println!("Sending subscription: {}\n", subscribe_msg);
    ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await?;

    println!("Waiting for messages...\n");
    let mut msg_count = 0;

    // Listen for messages
    while msg_count < 10 {
        if let Some(msg) = ws_stream.next().await {
            match msg? {
                Message::Text(text) => {
                    msg_count += 1;
                    println!("Message {}: ", msg_count);

                    // Parse and display
                    if let Ok(json) = serde_json::from_str::<Value>(&text) {
                        if let Some(event) = json.get("event") {
                            println!("  Event: {}", event);
                            if event == "error" {
                                println!("  Full error: {}", text);
                            }
                        } else if json.get("data").is_some() {
                            println!("  ✅ Received market data!");
                            if let Some(arg) = json.get("arg") {
                                println!("  Channel: {}", arg);
                            }
                        } else {
                            println!("  Unknown message: {}", text);
                        }
                    }
                }
                Message::Ping(data) => {
                    println!("Received ping, sending pong");
                    ws_stream.send(Message::Pong(data)).await?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}