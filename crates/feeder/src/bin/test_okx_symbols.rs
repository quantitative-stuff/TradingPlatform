use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Testing OKX symbols validity...\n");

    // Test symbols that might not exist
    let test_symbols = vec![
        "BTC-USDT", "ETH-USDT", "SOL-USDT",  // These should work
        "WLFI-USDT", "PUMP-USDT", "PI-USDT",  // These might not exist
        "BTC-USDT-SWAP", "ETH-USDT-SWAP",     // Swap symbols
    ];

    let ws_url = "wss://ws.okx.com:8443/ws/v5/public";
    let (mut ws_stream, _) = connect_async(ws_url).await?;
    println!("Connected to OKX WebSocket\n");

    // Subscribe to each symbol individually to see which ones fail
    for symbol in &test_symbols {
        let subscribe_msg = json!({
            "op": "subscribe",
            "args": [{
                "channel": "trades",
                "instId": symbol
            }]
        });

        println!("Testing symbol: {}", symbol);
        ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await?;

        // Wait for response
        let mut received_response = false;
        let start = std::time::Instant::now();

        while !received_response && start.elapsed() < std::time::Duration::from_secs(2) {
            if let Ok(Some(Ok(Message::Text(text)))) =
                tokio::time::timeout(std::time::Duration::from_millis(500), ws_stream.next()).await
            {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    if let Some(event) = value.get("event").and_then(Value::as_str) {
                        match event {
                            "subscribe" => {
                                println!("  ✅ SUCCESS: {} exists on OKX", symbol);
                                received_response = true;
                            }
                            "error" => {
                                let msg = value.get("msg").and_then(Value::as_str).unwrap_or("unknown");
                                println!("  ❌ FAILED: {} - {}", symbol, msg);
                                received_response = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if !received_response {
            println!("  ⚠️  No response for {}", symbol);
        }

        println!();
    }

    Ok(())
}