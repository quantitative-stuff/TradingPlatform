// Simple test to check Bybit WebSocket connection
use tokio_tungstenite::{connect_async, tungstenite::{Message, protocol::WebSocketConfig}};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use anyhow::Result;

#[tokio::main]
async fn main() {
    println!("Testing Bybit WebSocket connection...");
    
    // Test with spot WebSocket
    let url = "wss://stream.bybit.com/v5/public/spot";
    println!("Connecting to: {}", url);
    
    match connect_async(url).await {
        Ok((mut ws_stream, response)) => {
            println!("✅ Connected to Bybit!");
            println!("Response: {:?}", response);
            
            // Subscribe to BTC/USDT trade data
            let subscribe_msg = json!({
                "op": "subscribe",
                "args": ["publicTrade.BTCUSDT"]
            });
            
            println!("Sending subscription: {}", subscribe_msg);
            if let Err(e) = ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await {
                println!("❌ Failed to send subscription: {}", e);
                return;
            }
            
            println!("Waiting for messages...");
            let mut msg_count = 0;
            
            // Listen for messages
            while let Some(msg) = ws_stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        msg_count += 1;
                        println!("Message #{}: {}", msg_count, text);
                        
                        if msg_count >= 5 {
                            println!("✅ Test successful! Received {} messages", msg_count);
                            break;
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        println!("Received ping, sending pong");
                        let _ = ws_stream.send(Message::Pong(data)).await;
                    }
                    Ok(Message::Close(_)) => {
                        println!("❌ Connection closed by server");
                        break;
                    }
                    Err(e) => {
                        println!("❌ Error receiving message: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to connect: {}", e);
            println!("Error details: {:?}", e);
        }
    }
}