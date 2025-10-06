use async_trait::async_trait;
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use serde::Deserialize;

use crate::core::{MarketData, MarketDataType, TradeData, OrderBookData, get_optimized_udp_sender, CONNECTION_STATS};
use crate::stock::{Stock, Future, ContractMonth, StockFutureMapping};
use crate::error::Result;
use crate::load_config_ls::LSConfig;
use crate::stock::feed::FeederStock;
use tracing::{info, warn, error, debug};

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}


pub struct LSExchange {
    config: LSConfig,
    access_token: Option<String>,
    ws_stream: Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    symbols: Vec<String>,
    reconnect_count: u32,
}

impl StockFutureMapping {
    fn new(stock: Stock) -> Self {
        Self {
            stock,
            futures: HashMap::new(),
        }
    }

    fn add_future(&mut self, contract: ContractMonth, future: Future) {
        self.futures.insert(contract, future);
    }

    fn get_subscription_channels(&self) -> Vec<(String, &str)> {
        let mut channels = vec![
            // Stock channels
            (self.stock.code.clone(), "S3_"),  // Price
            (self.stock.code.clone(), "H1_"),  // Trade
        ];

        // Future channels
        for (_, future) in &self.futures {
            channels.push((future.code.clone(), "JC0")); // Future price
            channels.push((future.code.clone(), "JH0")); // Future trade
        }

        channels
    }
}


impl LSExchange {
    const BASE_URL: &'static str = "https://openapi.ls-sec.co.kr:8080";
    const WS_URL: &'static str = "wss://openapi.ls-sec.co.kr:9443/websocket";

    pub fn new(config: LSConfig, symbols: Vec<String>) -> Self {
        Self {
            config,
            access_token: None,
            ws_stream: None,
            symbols,
            reconnect_count: 0,
        }
    }

    async fn authenticate(&mut self) -> Result<()> {
        let client = reqwest::Client::new();

        // Ensure you handle the case where app_key or secret_key might be None
        // let app_key = &self.config.app_key;
        // let secret_key = &self.config.secret_key;

        let params = [
            ("grant_type", "client_credentials"),
            ("appkey", &self.config.app_key),
            ("appsecretkey", &self.config.secret_key),
            ("scope", "oob"),
        ];

        let response = client
            .post(format!("{}/oauth2/token", Self::BASE_URL))
            .form(&params)
            .send()
            .await?;

        // Print response status for debugging
        info!("Auth Status: {}", response.status());

        // Get the response text
        let text = response.text().await?;
        debug!("Auth Response: {}", text);

        // Parse the token response
        match serde_json::from_str::<TokenResponse>(&text) {
            Ok(token_response) => {
                self.access_token = Some(token_response.access_token);
                Ok(())
            },
            Err(e) => {
                Err(crate::error::Error::Auth(format!("Failed to parse token response: {}. Raw response: {}", e, text)))
            }
        }
    }

    async fn connect_websocket(&mut self) -> Result<()> {
        info!("Connecting to WebSocket URL: {}", Self::WS_URL);
        match connect_async(Self::WS_URL).await {
            Ok((ws_stream, response)) => {
                info!("WebSocket connected successfully! Response: {:?}", response);
                self.ws_stream = Some(ws_stream);
                Ok(())
            }
            Err(e) => {
                error!("WebSocket connection failed: {}", e);
                Err(crate::error::Error::Connection(format!("WebSocket connection failed: {}", e)))
            }
        }
    }

    fn create_subscription(&self) -> Result<String> {
        let access_token = self.access_token.as_ref()
            .ok_or_else(|| crate::error::Error::Auth("No access token".into()))?;

        let mut messages = Vec::new();

        // Subscribe to symbols from config (passed via constructor)
        for symbol in &self.symbols {
            // Determine transaction codes based on symbol pattern
            // Stock codes are 6 digits (e.g., "005930")
            // Future codes start with digits and contain letters (e.g., "101V6000")
            let is_future = symbol.chars().any(|c| c.is_alphabetic());

            if is_future {
                // Future subscription (JC0 for price, JH0 for trades)
                for tr_cd in ["JC0", "JH0"] {
                    messages.push(json!({
                        "header": {
                            "token": access_token,
                            "tr_type": "3"
                        },
                        "body": {
                            "tr_cd": tr_cd,
                            "tr_key": symbol
                        }
                    }));
                }
            } else {
                // Stock subscription (S3_ for price, H1_ for trades)
                for tr_cd in ["S3_", "H1_"] {
                    messages.push(json!({
                        "header": {
                            "token": access_token,
                            "tr_type": "3"
                        },
                        "body": {
                            "tr_cd": tr_cd,
                            "tr_key": symbol
                        }
                    }));
                }
            }
        }

        // Combine all messages into a single string, separated by newlines
        Ok(messages.into_iter()
            .map(|msg| msg.to_string())
            .collect::<Vec<_>>()
            .join("\n"))
    }


    async fn process_message(&self, message: Value) -> Result<()> {
        // Parse the message structure based on LS Securities format
        if let Some(header) = message.get("header") {
            if let Some(tr_cd) = header.get("tr_cd").and_then(|v| v.as_str()) {
                match tr_cd {
                    "S3_" => {
                        // Stock price/quote data with bid/ask
                        self.handle_stock_price(message).await?;
                    },
                    "H1_" => {
                        // Stock trade data
                        self.handle_stock_trade(message).await?;
                    },
                    "JC0" => {
                        // Futures price data
                        self.handle_futures_price(message).await?;
                    },
                    "JH0" => {
                        // Futures trade data
                        self.handle_futures_trade(message).await?;
                    },
                    _ => {
                        debug!("Unknown message type: {}", tr_cd);
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_stock_price(&self, message: Value) -> Result<()> {
        // Parse stock price data and send via UDP
        if let Some(body) = message.get("body") {
            // Debug: uncomment to see body fields
            // println!("    📝 Body fields: {}", serde_json::to_string(&body).unwrap_or_default());
            
            let symbol = body.get("shcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            // Parse fields - they come as strings, not numbers
            let price = body.get("price")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            
            let change = body.get("change")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            
            let volume = body.get("volume")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            
            // Only log if you want to see the data flow
            // println!("    💰 {} - Price: {}, Change: {}, Volume: {}", symbol, price, change, volume);
            let bid_price = body.get("bidho")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let ask_price = body.get("offerho")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let bid_size = body.get("bidrem")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let ask_size = body.get("offerrem")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            
            // Create OrderBookData with bid/ask levels
            let orderbook_data = OrderBookData {
                exchange: "LS".to_string(),
                symbol: symbol.clone(),
                asset_type: "stock".to_string(),
                bids: vec![(bid_price, bid_size)],
                asks: vec![(ask_price, ask_size)],
                timestamp: chrono::Utc::now().timestamp_millis(),
            };

            let market_data = MarketData {
                exchange: "LS".to_string(),
                symbol,
                timestamp: chrono::Utc::now(),
                data_type: MarketDataType::OrderBook(orderbook_data),
            };

            // Send via UDP
            if let Some(sender) = get_optimized_udp_sender() {
                match &market_data.data_type {
                    MarketDataType::OrderBook(orderbook) => {
                        match sender.send_orderbook(orderbook.clone()) {
                            Ok(_) => {
                                // Debug: Show UDP send success
                                static UDP_COUNT: AtomicUsize = AtomicUsize::new(0);
                                let count = UDP_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                                if count % 50 == 1 {
                                    debug!("UDP: Sent {} orderbook packets", count);
                                }
                            },
                            Err(e) => {
                                warn!("Failed to send orderbook via UDP: {}", e);
                            }
                        }
                    },
                    _ => {}
                }
            } else {
                warn!("WARNING: UDP sender not initialized!");
            }
        }
        Ok(())
    }

    async fn handle_stock_trade(&self, message: Value) -> Result<()> {
        // Parse stock trade data and send via UDP
        if let Some(body) = message.get("body") {
            let symbol = body.get("shcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            let trade_data = TradeData {
                exchange: "LS".to_string(),
                symbol: symbol.clone(),
                asset_type: "stock".to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                price: body.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0),
                quantity: body.get("cvolume").and_then(|v| v.as_f64()).unwrap_or(0.0),
            };

            let market_data = MarketData {
                exchange: "LS".to_string(),
                symbol,
                timestamp: chrono::Utc::now(),
                data_type: MarketDataType::Trade(trade_data),
            };

            // Send via UDP
            if let Some(sender) = get_optimized_udp_sender() {
                match &market_data.data_type {
                    MarketDataType::Trade(trade) => {
                        match sender.send_trade(trade.clone()) {
                            Ok(_) => {
                                // Uncomment to debug
                                // debug!("Sent trade for {} via UDP", trade.symbol);
                            },
                            Err(e) => {
                                warn!("Failed to send trade via UDP: {}", e);
                            }
                        }
                    },
                    _ => {}
                }
            } else {
                warn!("WARNING: UDP sender not initialized!");
            }
        }
        Ok(())
    }

    async fn handle_futures_price(&self, message: Value) -> Result<()> {
        // Similar to handle_stock_price but for futures
        self.handle_stock_price(message).await
    }

    async fn handle_futures_trade(&self, message: Value) -> Result<()> {
        // Similar to handle_stock_trade but for futures
        self.handle_stock_trade(message).await
    }
}

#[async_trait]
impl FeederStock for LSExchange {
    fn name(&self) -> &str {
        "LS_sec"
    }   

    async fn connect(&mut self) -> Result<()> {
        // First authenticate to get the token
        self.authenticate().await?;
        
        // Then establish WebSocket connection
        self.connect_websocket().await?;
        
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<()> {
        info!("Starting subscription process...");
        // Check if we're authenticated and connected
        if self.access_token.is_none() {
            return Err(crate::error::Error::Auth("Not authenticated".into()));
        }

        let mut ws_stream = self.ws_stream.take()
        .ok_or_else(|| crate::error::Error::Connection("Not connected".into()))?;

        info!("Creating subscription messages...");
        let subscription = self.create_subscription()?;
        info!("Sending subscription ({} bytes)...", subscription.len());
        
        ws_stream.send(Message::text(subscription)).await?;
        
        self.ws_stream = Some(ws_stream);

        // Add delay between subscriptions
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        info!("Subscription sent successfully!");

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        let mut ws_stream = self.ws_stream.take()
            .ok_or_else(|| crate::error::Error::Connection("Not connected".into()))?;

        info!("Starting LS Securities WebSocket listener - waiting for messages...");
        {
            let mut stats = CONNECTION_STATS.write();
            let entry = stats.entry("LS".to_string()).or_insert_with(Default::default);
            entry.connected += 1;
            entry.total_connections += 1;
        }

        loop {
            tokio::select! {
                Some(msg) = ws_stream.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // Message counting could be added to ConnectionStats struct if needed
                            if let Ok(json_msg) = serde_json::from_str::<Value>(&text) {
                                // Simple counter to show data is flowing
                                static MSG_COUNT: AtomicUsize = AtomicUsize::new(0);
                                let count = MSG_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                                if count % 100 == 0 {
                                    debug!("Processed {} messages...", count);
                                }

                                if let Err(e) = self.process_message(json_msg).await {
                                    warn!("Error processing message: {}", e);
                                }
                            } else {
                                warn!("Failed to parse message as JSON");
                            }
                        },
                        Ok(Message::Ping(ping)) => {
                            ws_stream.send(Message::Pong(ping)).await?;
                        },
                        Ok(Message::Close(_)) => {
                            warn!("WebSocket connection closed by server");
                            break;
                        },
                        Err(e) => {
                            error!("WebSocket error: {}", e);
                            {
                                let mut stats = CONNECTION_STATS.write();
                                if let Some(entry) = stats.get_mut("LS") {
                                    entry.last_error = Some(e.to_string());
                                }
                            }
                            break;
                        },
                        _ => {}
                    }
                }
                else => {
                    error!("WebSocket stream ended unexpectedly");
                    break;
                }
            }
        }

        {
            let mut stats = CONNECTION_STATS.write();
            if let Some(entry) = stats.get_mut("LS") {
                if entry.connected > 0 {
                    entry.connected -= 1;
                }
                entry.disconnected += 1;
            }
        }
        
        // Attempt reconnection
        if self.reconnect_count < 5 {
            self.reconnect_count += 1;
            warn!("Attempting reconnection {} / 5", self.reconnect_count);
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            // Reconnect
            if let Err(e) = self.connect().await {
                error!("Failed to reconnect: {}", e);
            } else if let Err(e) = self.subscribe().await {
                error!("Failed to resubscribe: {}", e);
            } else {
                return self.start().await;
            }
        }

        Ok(())
    }
}