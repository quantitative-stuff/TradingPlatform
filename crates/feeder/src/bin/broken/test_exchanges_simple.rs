'''use tokio;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::json;
use colored::*;
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("{}", "========================================".bright_cyan());
    println!("{}", "  NEW EXCHANGES CONNECTION TEST".bright_white().bold());
    println!("{}", "========================================".bright_cyan());
    println!();

    println!("Select exchange to test:");
    println!("1. Coinbase");
    println!("2. Deribit");
    println!("3. OKX");
    println!("4. Bithumb");
    println!("5. Test all");
    print!("Enter choice (1-5): ");
    
    use std::io::{self, Write};
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let choice = input.trim().parse::<u32>().unwrap_or(5);
    
    println!();
    
    match choice {
        1 => test_coinbase().await,
        2 => test_deribit().await,
        3 => test_okx().await,
        4 => test_bithumb().await,
        5 => {
            test_coinbase().await;
            println!();
            test_deribit().await;
            println!();
            test_okx().await;
            println!();
            test_bithumb().await;
        }
        _ => println!("Invalid choice"),
    }
}

async fn test_coinbase() {
    println!("{}", "Testing COINBASE...".bright_green().bold());
    
    let url = "wss://advanced-trade-ws.coinbase.com";
    
    match connect_async(url).await {
        Ok((mut ws_stream, _)) => {
            println!("{} Connected to Coinbase!", "✓".green());
            
            // Subscribe to BTC-USD
            let subscribe_msg = json!({
                "type": "subscribe",
                "product_ids": ["BTC-USD", "ETH-USD"],
                "channels": ["heartbeat", "ticker"]
            });
            
            ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await.unwrap();
            println!("Sent subscription request...");
            
            // Read a few messages
            let mut count = 0;
            while let Some(msg) = ws_stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if text.contains("subscriptions") {
                            println!("{} Subscription confirmed!", "✓".green());
                        } else if text.contains("heartbeat") {
                            println!("{} Heartbeat received", "♥".red());
                        } else if text.contains("ticker") {
                            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({}));
                            if let Some(price) = v.get("price") {
                                println!("  Price update: {}", price.to_string().yellow());
                            }
                        }
                        
                        count += 1;
                        if count >= 5 {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        println!("{} Connection closed", "✗".red());
                        break;
                    }
                    Err(e) => {
                        println!("{} Error: {}", "✗".red(), e);
                        break;
                    }
                    _ => {}
                }
            }
            
            ws_stream.close(None).await.ok();
            println!("{} Coinbase test complete", "✓".green());
        }
        Err(e) => {
            println!("{} Failed to connect to Coinbase: {}", "✗".red(), e);
        }
    }
}

async fn test_deribit() {
    println!("{}", "Testing DERIBIT...".bright_green().bold());
    
    let url = "wss://www.deribit.com/ws/api/v2";
    
    match connect_async(url).await {
        Ok((mut ws_stream, _)) => {
            println!("{} Connected to Deribit!", "✓".green());
            
            // Subscribe to BTC perpetual
            let subscribe_msg = json!({
                "jsonrpc": "2.0",
                "method": "public/subscribe",
                "params": {
                    "channels": ["trades.BTC-PERPETUAL.raw", "ticker.BTC-PERPETUAL.raw"]
                },
                "id": 1
            });
            
            ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await.unwrap();
            println!("Sent subscription request...");
            
            // Read a few messages
            let mut count = 0;
            while let Some(msg) = ws_stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({}));
                        
                        if v.get("result").is_some() {
                            println!("{} Subscription confirmed!", "✓".green());
                        } else if let Some(params) = v.get("params") {
                            if let Some(channel) = params.get("channel") {
                                let channel_str = channel.as_str().unwrap_or("");
                                if channel_str.contains("trades") {
                                    if let Some(data) = params.get("data") {
                                        if let Some(trades) = data.as_array() {
                                            for trade in trades.iter().take(1) {
                                                if let Some(price) = trade.get("price") {
                                                    println!("  Trade: {} @ {}", 
                                                        "BTC-PERPETUAL".cyan(),
                                                        price.to_string().yellow()
                                                    );
                                                }
                                            }
                                        }
                                    }
                                } else if channel_str.contains("ticker") {
                                    if let Some(data) = params.get("data") {
                                        if let Some(last_price) = data.get("last_price") {
                                            println!("  Ticker: {}", last_price.to_string().yellow());
                                        }
                                    }
                                }
                            }
                        }
                        
                        count += 1;
                        if count >= 5 {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        println!("{} Connection closed", "✗".red());
                        break;
                    }
                    Err(e) => {
                        println!("{} Error: {}", "✗".red(), e);
                        break;
                    }
                    _ => {}
                }
            }
            
            ws_stream.close(None).await.ok();
            println!("{} Deribit test complete", "✓".green());
        }
        Err(e) => {
            println!("{} Failed to connect to Deribit: {}", "✗".red(), e);
        }
    }
}

async fn test_okx() {
    println!("{}", "Testing OKX...".bright_green().bold());
    
    let url = "wss://ws.okx.com:8443/ws/v5/public";
    
    match connect_async(url).await {
        Ok((mut ws_stream, _)) => {
            println!("{} Connected to OKX!", "✓".green());
            
            // Subscribe to BTC spot
            let subscribe_msg = json!({
                "op": "subscribe",
                "args": [
                    {
                        "channel": "trades",
                        "instId": "BTC-USDT"
                    },
                    {
                        "channel": "tickers",
                        "instId": "BTC-USDT"
                    }
                ]
            });
            
            ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await.unwrap();
            println!("Sent subscription request...");
            
            // Read a few messages
            let mut count = 0;
            while let Some(msg) = ws_stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({}));
                        
                        if let Some(event) = v.get("event") {
                            if event == "subscribe" {
                                println!("{} Subscription confirmed!", "✓".green());
                            }
                        } else if let Some(arg) = v.get("arg") {
                            if let Some(channel) = arg.get("channel") {
                                let channel_str = channel.as_str().unwrap_or("");
                                if channel_str == "trades" {
                                    if let Some(data) = v.get("data") {
                                        if let Some(trades) = data.as_array() {
                                            for trade in trades.iter().take(1) {
                                                if let Some(px) = trade.get("px") {
                                                    println!("  Trade: BTC-USDT @ {}", 
                                                        px.to_string().yellow()
                                                    );
                                                }
                                            }
                                        }
                                    }
                                } else if channel_str == "tickers" {
                                    if let Some(data) = v.get("data") {
                                        if let Some(tickers) = data.as_array() {
                                            for ticker in tickers.iter().take(1) {
                                                if let Some(last) = ticker.get("last") {
                                                    println!("  Ticker: {}", last.to_string().yellow());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        
                        count += 1;
                        if count >= 5 {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        println!("{} Connection closed", "✗".red());
                        break;
                    }
                    Err(e) => {
                        println!("{} Error: {}", "✗".red(), e);
                        break;
                    }
                    _ => {}
                }
            }
            
            ws_stream.close(None).await.ok();
            println!("{} OKX test complete", "✓".green());
        }
        Err(e) => {
            println!("{} Failed to connect to OKX: {}", "✗".red(), e);
        }
    }
}

async fn test_bithumb() {
    println!("{}", "Testing BITHUMB...".bright_green().bold());
    
    let url = "wss://pubwss.bithumb.com/pub/ws";
    
    match connect_async(url).await {
        Ok((mut ws_stream, _)) => {
            println!("{} Connected to Bithumb!", "✓".green());
            
            // Subscribe to BTC KRW
            let subscribe_msg = json!({
                "type": "transaction",
                "symbols": ["BTC_KRW"]
            });
            
            ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await.unwrap();
            println!("Sent subscription request...");
            
            // Also subscribe to ticker
            let ticker_msg = json!({
                "type": "ticker",
                "symbols": ["BTC_KRW"],
                "tickTypes": ["24H"]
            });
            ws_stream.send(Message::Text(ticker_msg.to_string().into())).await.unwrap();
            
            // Read a few messages
            let mut count = 0;
            tokio::time::timeout(Duration::from_secs(10), async {
                while let Some(msg) = ws_stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({}));
                            
                            if let Some(status) = v.get("status") {
                                if status == "0000" {
                                    println!("{} Response received", "✓".green());
                                }
                            }
                            
                            if let Some(type_field) = v.get("type") {
                                let type_str = type_field.as_str().unwrap_or("");
                                if type_str == "transaction" {
                                    if let Some(content) = v.get("content") {
                                        if let Some(list) = content.get("list") {
                                            if let Some(trades) = list.as_array() {
                                                for trade in trades.iter().take(1) {
                                                    if let Some(price) = trade.get("contPrice") {
                                                        println!("  Trade: BTC_KRW @ {} KRW", 
                                                            price.to_string().yellow()
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else if type_str == "ticker" {
                                    println!("  Ticker data received");
                                }
                            }
                            
                            count += 1;
                            if count >= 3 {
                                break;
                            }
                        }
                        Ok(Message::Close(_)) => {
                            println!("{} Connection closed", "✗".red());
                            break;
                        }
                        Err(e) => {
                            println!("{} Error: {}", "✗".red(), e);
                            break;
                        }
                        _ => {}
                    }
                }
            }).await.ok();
            
            ws_stream.close(None).await.ok();
            println!("{} Bithumb test complete", "✓".green());
        }
        Err(e) => {
            println!("{} Failed to connect to Bithumb: {}", "✗".red(), e);
        }
    }
}
'''