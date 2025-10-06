use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::json;
use colored::*;

#[tokio::main]
async fn main() {
    println!("{}", "🔍 TESTING EXCHANGE WEBSOCKET CONNECTIONS".bright_cyan().bold());
    println!("{}", "=".repeat(60));
    
    // Test OKX
    println!("\n{} Testing OKX...", "1️⃣".bright_white());
    test_okx().await;
    
    // Test Bybit
    println!("\n{} Testing Bybit...", "2️⃣".bright_white());
    test_bybit().await;
    
    // Test Coinbase
    println!("\n{} Testing Coinbase...", "3️⃣".bright_white());
    test_coinbase().await;
    
    // Test Deribit
    println!("\n{} Testing Deribit...", "4️⃣".bright_white());
    test_deribit().await;
}

async fn test_okx() {
    let url = "wss://ws.okx.com:8443/ws/v5/public";
    println!("   URL: {}", url.yellow());
    
    match connect_async(url).await {
        Ok((mut ws, _)) => {
            println!("   {} Connected!", "✅".green());
            
            // Subscribe to BTC-USDT trades
            let sub = json!({
                "op": "subscribe",
                "args": [{
                    "channel": "trades",
                    "instId": "BTC-USDT"
                }]
            });
            
            println!("   Subscribing to BTC-USDT trades...");
            if let Err(e) = ws.send(Message::Text(sub.to_string().into())).await {
                println!("   {} Subscribe failed: {}", "❌".red(), e);
                return;
            }
            
            // Wait for response
            if let Ok(msg) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ws.next()
            ).await {
                if let Some(Ok(Message::Text(text))) = msg {
                    println!("   Response: {}", text.bright_green());
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json["event"] == "subscribe" {
                            println!("   {} Subscription successful!", "✅".green());
                        } else if json["event"] == "error" {
                            println!("   {} Error: {}", "❌".red(), json["msg"]);
                        }
                    }
                }
            } else {
                println!("   {} Timeout waiting for response", "⏱️".yellow());
            }
        }
        Err(e) => {
            println!("   {} Connection failed: {}", "❌".red(), e);
        }
    }
}

async fn test_bybit() {
    let url = "wss://stream.bybit.com/v5/public/spot";
    println!("   URL: {}", url.yellow());
    
    match connect_async(url).await {
        Ok((mut ws, _)) => {
            println!("   {} Connected!", "✅".green());
            
            // Subscribe to BTC/USDT trades
            let sub = json!({
                "op": "subscribe",
                "args": ["publicTrade.BTCUSDT"]
            });
            
            println!("   Subscribing to BTCUSDT trades...");
            if let Err(e) = ws.send(Message::Text(sub.to_string().into())).await {
                println!("   {} Subscribe failed: {}", "❌".red(), e);
                return;
            }
            
            // Wait for response
            if let Ok(msg) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ws.next()
            ).await {
                if let Some(Ok(Message::Text(text))) = msg {
                    println!("   Response: {}", text.bright_green());
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json["success"] == true {
                            println!("   {} Subscription successful!", "✅".green());
                        } else {
                            println!("   {} Failed: {}", "❌".red(), json["ret_msg"]);
                        }
                    }
                }
            } else {
                println!("   {} Timeout waiting for response", "⏱️".yellow());
            }
        }
        Err(e) => {
            println!("   {} Connection failed: {}", "❌".red(), e);
        }
    }
}

async fn test_coinbase() {
    let url = "wss://advanced-trade-ws.coinbase.com";
    println!("   URL: {}", url.yellow());
    
    match connect_async(url).await {
        Ok((mut ws, _)) => {
            println!("   {} Connected!", "✅".green());
            
            // Subscribe to BTC-USD trades
            let sub = json!({
                "type": "subscribe",
                "product_ids": ["BTC-USD"],
                "channel": "ticker"
            });
            
            println!("   Subscribing to BTC-USD ticker...");
            if let Err(e) = ws.send(Message::Text(sub.to_string().into())).await {
                println!("   {} Subscribe failed: {}", "❌".red(), e);
                return;
            }
            
            // Wait for response
            if let Ok(msg) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ws.next()
            ).await {
                if let Some(Ok(Message::Text(text))) = msg {
                    println!("   Response: {}", text.bright_green());
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json["type"] == "subscriptions" {
                            println!("   {} Subscription successful!", "✅".green());
                        } else if json["type"] == "error" {
                            println!("   {} Error: {}", "❌".red(), json["message"]);
                        }
                    }
                }
            } else {
                println!("   {} Timeout waiting for response", "⏱️".yellow());
            }
        }
        Err(e) => {
            println!("   {} Connection failed: {}", "❌".red(), e);
        }
    }
}

async fn test_deribit() {
    let url = "wss://www.deribit.com/ws/api/v2";  // Note: v2 is current
    println!("   URL: {}", url.yellow());
    
    match connect_async(url).await {
        Ok((mut ws, _)) => {
            println!("   {} Connected!", "✅".green());
            
            // Subscribe to BTC trades
            let sub = json!({
                "jsonrpc": "2.0",
                "method": "public/subscribe",
                "params": {
                    "channels": ["trades.BTC-PERPETUAL.raw"]
                },
                "id": 1
            });
            
            println!("   Subscribing to BTC-PERPETUAL trades...");
            if let Err(e) = ws.send(Message::Text(sub.to_string().into())).await {
                println!("   {} Subscribe failed: {}", "❌".red(), e);
                return;
            }
            
            // Wait for response
            if let Ok(msg) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ws.next()
            ).await {
                if let Some(Ok(Message::Text(text))) = msg {
                    println!("   Response: {}", text.bright_green());
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if json["result"].is_array() {
                            println!("   {} Subscription successful!", "✅".green());
                        } else if json["error"].is_object() {
                            println!("   {} Error: {}", "❌".red(), json["error"]["message"]);
                        }
                    }
                }
            } else {
                println!("   {} Timeout waiting for response", "⏱️".yellow());
            }
        }
        Err(e) => {
            println!("   {} Connection failed: {}", "❌".red(), e);
        }
    }
}