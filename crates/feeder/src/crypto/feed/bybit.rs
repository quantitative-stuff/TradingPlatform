use async_trait::async_trait;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use super::websocket_config::connect_with_large_buffer;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn, error};
use futures_util::future::FutureExt;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::{Feeder, SymbolMapper, CONNECTION_STATS, get_shutdown_receiver, get_multi_port_sender, parse_to_scaled_or_default};
use crate::error::{Result, Error};
use crate::load_config::ExchangeConfig;
use crate::connect_to_databse::ConnectionEvent;

const USE_MULTI_PORT_UDP: bool = true;

pub struct BybitExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    // Map of (asset_type, chunk_index) -> WebSocket connection
    // Each connection handles up to 5 symbols with both trade and orderbook streams
    ws_streams: HashMap<(String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    // Track active connections for proper reconnection
    active_connections: Arc<AtomicUsize>,
}

impl BybitExchange {
    pub fn new(config: ExchangeConfig, symbol_mapper: Arc<SymbolMapper>) -> Self {
        const SYMBOLS_PER_CONNECTION: usize = 5;
        let mut ws_streams = HashMap::new();
        
        // Initialize connections for each asset type
        for asset_type in &config.feed_config.asset_type {
            let symbols = if asset_type == "spot" {
                &config.subscribe_data.spot_symbols
            } else {
                &config.subscribe_data.futures_symbols
            };

            let num_symbols = symbols.len();
            let num_chunks = (num_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;

            for chunk_idx in 0..num_chunks {
                ws_streams.insert(
                    (asset_type.clone(), chunk_idx),
                    None
                );
            }
        }
        
        Self {
            config,
            symbol_mapper,
            ws_streams,
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }

    async fn get_ws_url(&self, asset_type: &str) -> Result<String> {
        let url = match asset_type {
            "spot" => self.config.connect_config.ws_url.clone(),
            "linear" => self.config.connect_config.ws_url.replace("/spot", "/linear"),
            "inverse" => self.config.connect_config.ws_url.replace("/spot", "/inverse"),
            _ => return Err(Error::InvalidInput(format!("Invalid asset type: {}", asset_type))),
        };
        Ok(url)
    }


}

#[async_trait]
impl Feeder for BybitExchange {
    fn name(&self) -> &str {
        "Bybit"
    }

    async fn connect(&mut self) -> Result<()> {
        let config = self.config.clone();  // Clone config
        
        info!("Bybit: Creating {} WebSocket connections (5 symbols each, with both trade and orderbook streams)", self.ws_streams.len());
        
        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            // Map to Bybit-specific asset types
            let ws_url = match asset_type.as_str() {
                "spot" => "wss://stream.bybit.com/v5/public/spot".to_string(),
                "linear" => "wss://stream.bybit.com/v5/public/linear".to_string(),
                _ => return Err(Error::InvalidInput(format!("Invalid asset type: {}", asset_type))),
            };
            // Only log at debug level
            debug!("Connecting to Bybit {} chunk {}: {}", asset_type, chunk_idx, ws_url);
            
            // Add retry logic with exponential backoff for errors
            let mut retry_count = 0;
            let max_retries = 5; // Increased from 3 to 5
            let mut retry_delay = Duration::from_secs(config.connect_config.initial_retry_delay_secs);
            
            loop {
                match connect_with_large_buffer(&ws_url).await {
                    Ok((stream, _)) => {
                        *ws_stream = Some(stream);
                        info!("Successfully connected to Bybit {} chunk {} at {}", 
                            asset_type, chunk_idx, ws_url);
                        
                        // Update connection stats - mark as connected
                        {
                            let mut stats = CONNECTION_STATS.write();
                            let key = format!("Bybit_{}", asset_type);
                            let entry = stats.entry(key.clone()).or_default();
                            entry.connected += 1;
                            entry.total_connections += 1;
                            info!("Bybit: [{}] Connection established for chunk {}", key, chunk_idx);
                        }
                        
                        // Log successful connection to QuestDB
                        let connection_id = format!("bybit-{}-{}", asset_type, chunk_idx);
                        let _ = crate::core::feeder_metrics::log_connection(
                            "Bybit", 
                            &connection_id, 
                            ConnectionEvent::Connected
                        ).await;
                        
                        debug!("Connected to Bybit {} chunk {}", asset_type, chunk_idx);
                        break;
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        retry_count += 1;
                        
                        // Check various error conditions
                        if error_str.contains("403") || error_str.contains("Forbidden") {
                            warn!("Got 403 error from Bybit {} chunk {}. Retrying in {} seconds (attempt {}/{})", 
                                asset_type, chunk_idx, retry_delay.as_secs(), retry_count, max_retries);
                            // Increase delay significantly for 403 errors
                            retry_delay = retry_delay.saturating_mul(3).min(Duration::from_secs(600));
                        } else if error_str.contains("TLS error") || error_str.contains("unexpected EOF") {
                            // TLS errors often mean rate limiting or connection limit
                            warn!("TLS/Connection error for Bybit {} chunk {}. Likely rate limited. Retrying in {} seconds (attempt {}/{})", 
                                asset_type, chunk_idx, retry_delay.as_secs(), retry_count, max_retries);
                            retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(300));
                        } else if error_str.contains("429") || error_str.contains("Too Many") {
                            warn!("Rate limit hit for Bybit {} chunk {}. Retrying in {} seconds (attempt {}/{})", 
                                asset_type, chunk_idx, retry_delay.as_secs(), retry_count, max_retries);
                            retry_delay = retry_delay.saturating_mul(3).min(Duration::from_secs(600));
                        } else {
                            warn!("Connection error for Bybit {} chunk {}: {}. Retrying in {} seconds (attempt {}/{})", 
                                asset_type, chunk_idx, error_str, retry_delay.as_secs(), retry_count, max_retries);
                        }
                        
                        if retry_count > max_retries {
                            warn!("Failed to connect to Bybit {} chunk {} after {} retries. Skipping this connection.", 
                                asset_type, chunk_idx, max_retries);
                            // Skip this connection instead of failing entirely
                            *ws_stream = None;
                            break;
                        }
                        
                        // Wait before retrying with exponential backoff
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(config.connect_config.max_retry_delay_secs));
                    }
                }
            }
            // Use longer delay between connections for Bybit to avoid rate limits
            let connection_delay = Duration::from_millis(config.connect_config.connection_delay_ms.max(1000)); // Minimum 1 second
            tokio::time::sleep(connection_delay).await;
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<()> {
        const SYMBOLS_PER_CONNECTION: usize = 5;
        let config = self.config.clone();  // Clone config
        
        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            if let Some(ws_stream) = ws_stream {
                // Get symbols based on asset type
                let symbols = if asset_type == "spot" {
                    &config.subscribe_data.spot_symbols
                } else {
                    &config.subscribe_data.futures_symbols
                };

                // Get the symbols for this chunk (5 symbols per connection)
                let start_idx = *chunk_idx * SYMBOLS_PER_CONNECTION;
                let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, symbols.len());
                let chunk_symbols = &symbols[start_idx..end_idx];
                
                // Create subscription topics for BOTH trade and orderbook for each symbol
                let mut topics = Vec::new();
                for code in chunk_symbols {
                    let formatted_symbol = code.clone();

                    // Add both trade and orderbook topics for each symbol
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

                let subscription = subscribe_msg.to_string();
                debug!("Sending subscription to Bybit {} chunk {} with {} symbols ({} topics total)", 
                    asset_type, chunk_idx, chunk_symbols.len(), topics.len());
                
                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;
                debug!("Bybit: Sent subscription to {} chunk {} with {} symbols",
                    asset_type, chunk_idx, chunk_symbols.len());
                debug!("  Subscribed symbols: {:?}", chunk_symbols);
                debug!("Subscribed to Bybit {} chunk {}", asset_type, chunk_idx);
            }
        }
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        // Reset connection counter
        self.active_connections.store(0, Ordering::SeqCst);

        for ((asset_type, chunk_idx), ws_stream_opt) in self.ws_streams.iter_mut() {
            if let Some(mut ws_stream) = ws_stream_opt.take() {
                let asset_type = asset_type.clone();
                let chunk_idx = *chunk_idx;
                let symbol_mapper = self.symbol_mapper.clone();
                let active_connections = self.active_connections.clone();
                let config = self.config.clone();
                
                // Increment active connection count
                active_connections.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    info!("Bybit: Starting {} chunk {} stream processing (trade + orderbook)",
                        asset_type, chunk_idx);
                    
                    // Get shutdown receiver
                    let mut shutdown_rx = get_shutdown_receiver();
                    
                    // Connection tracking is handled in feeder.rs, not here
                    
                    // Create a ping interval to keep connection alive - more frequent pings
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(15)); // Reduced from 20 to 15 seconds
                    
                    // Add a connection health check
                    let mut last_message_time = std::time::Instant::now();
                    let mut health_check_interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute
                    
                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Bybit {} chunk {} received shutdown signal", asset_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                // Send ping to keep connection alive
                                if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Bybit {} chunk {}: {}", 
                                        asset_type, chunk_idx, e);
                                    
                                    // Connection tracking is handled in feeder.rs
                                    // Log disconnection/error to file
                                    error!("[Bybit_{}] WebSocket ping failed for chunk {}", asset_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to Bybit {} chunk {}", 
                                    asset_type, chunk_idx);
                            }
                            _ = health_check_interval.tick() => {
                                // Check if we haven't received any messages for too long
                                if last_message_time.elapsed() > Duration::from_secs(300) { // 5 minutes
                                    warn!("No messages received from Bybit {} chunk {} for {} seconds, connection may be stale", 
                                        asset_type, chunk_idx, last_message_time.elapsed().as_secs());
                                    
                                    // Send a test ping to check connection health
                                    if let Err(e) = ws_stream.send(Message::Ping(b"health_check".to_vec().into())).await {
                                        error!("Health check ping failed for Bybit {} chunk {}: {}", 
                                            asset_type, chunk_idx, e);
                                        break;
                                    }
                                }
                            }
                            msg = ws_stream.next().fuse() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        last_message_time = std::time::Instant::now(); // Update last message time
                                        process_bybit_message(&text, symbol_mapper.clone(), &asset_type, &config);
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        last_message_time = std::time::Instant::now(); // Update last message time
                                        debug!("Received pong from Bybit {} chunk {}", 
                                            asset_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Ping(data))) => {
                                        last_message_time = std::time::Instant::now(); // Update last message time
                                        // Respond to server ping with pong
                                        if let Err(e) = ws_stream.send(Message::Pong(data)).await {
                                            warn!("Failed to send pong to Bybit {} chunk {}: {}", 
                                                asset_type, chunk_idx, e);
                                            break;
                                        }
                                        debug!("Sent pong response to Bybit {} chunk {}", 
                                            asset_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("Bybit WebSocket closed for {} chunk {}: {:?}", 
                                            asset_type, chunk_idx, frame);
                                        
                                        // Update connection stats - mark as disconnected
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Bybit_{}", asset_type);
                                            let entry = stats.entry(key.clone()).or_default();
                                            if entry.connected > 0 {
                                                entry.connected -= 1;
                                                entry.disconnected += 1;
                                            }
                                            // Log disconnection to file
                                            error!("[{}] WebSocket closed for chunk {}: {:?}", key, chunk_idx, frame);
                                        }
                                        break;
                                    },
                                    Some(Ok(msg)) => {
                                        debug!("Received other message type from Bybit: {:?}", msg);
                                    },
                                    Some(Err(e)) => {
                                        // Don't print to stderr in silent mode, use warn! instead
                                        warn!("Bybit WebSocket error for {} chunk {}: {}", 
                                            asset_type, chunk_idx, e);
                                        
                                        // Update connection stats - mark as disconnected
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Bybit_{}", asset_type);
                                            let entry = stats.entry(key.clone()).or_default();
                                            if entry.connected > 0 {
                                                entry.connected -= 1;
                                                entry.disconnected += 1;
                                            }
                                            // Log disconnection to file
                                            error!("[{}] WebSocket error for chunk {}: {}", key, chunk_idx, e);
                                        }
                                        break;
                                    },
                                    None => {
                                        warn!("WebSocket stream ended for Bybit {} chunk {}", 
                                            asset_type, chunk_idx);
                                        
                                        // Update connection stats - mark as disconnected
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Bybit_{}", asset_type);
                                            let entry = stats.entry(key.clone()).or_default();
                                            if entry.connected > 0 {
                                                entry.connected -= 1;
                                                entry.disconnected += 1;
                                            }
                                            // Log disconnection to file
                                            error!("[{}] WebSocket stream ended for chunk {}", key, chunk_idx);
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    warn!("WebSocket connection lost for Bybit {} chunk {}, will be retried by feeder", 
                        asset_type, chunk_idx);
                    
                    // Decrement active connection count
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
        }

        // Wait until all connections are disconnected instead of infinite loop
        while self.active_connections.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        warn!("All Bybit connections lost, returning to trigger reconnection");
        Ok(())
    }
}



fn process_bybit_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, config: &ExchangeConfig) {
    // Add debug to see all messages
    if !text.contains("pong") && !text.contains("ping") {
        debug!("{}", crate::core::safe_websocket_message_log("Bybit", text));
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", crate::core::safe_json_parse_error_log(&e, text));
            return;
        }
    };

    // Handle subscription responses
    if let Some(op) = value.get("op") {
        if op == "subscribe" {
            // Log subscription responses
            if let Some(success) = value.get("success").and_then(|v| v.as_bool()) {
                if success {
                    info!("[Bybit] Subscription successful for asset_type: {}", asset_type);
                } else {
                    if let Some(msg) = value.get("ret_msg").and_then(|v| v.as_str()) {
                        // Only log at debug level for invalid topics (common and expected)
                        if msg.contains("Invalid topic") || msg.contains("handler not found") {
                            warn!("[Bybit] Subscription failed (invalid topic): {}", msg);
                        } else {
                            error!("[Bybit] Subscription failed: {}", msg);
                        }
                    }
                }
            }
            return;
        }
    }

    if let Some(topic) = value.get("topic").and_then(Value::as_str) {
        // println!("[Bybit][{}] Topic: {}", stream_type, topic);
        if topic.starts_with("publicTrade") {
            // Handle trade messages.
            let data = &value["data"];
            if data.is_array() {
                // Snapshot: data is an array of trade objects.
                for trade in data.as_array().unwrap() {
                    process_bybit_trade(trade, &symbol_mapper, asset_type, config);
                }
            } else if data.is_object() {
                // Delta: data is a single trade object.
                process_bybit_trade(data, &symbol_mapper, asset_type, config);
            } else {
                debug!("[Bybit] Unexpected data format in trade message.");
            }
        } else if topic.starts_with("orderbook") {
            // Handle orderbook messages.
            let data = &value["data"];

            // Check message type - should be "snapshot" initially, then "delta"
            let message_type = data.get("type").and_then(Value::as_str).unwrap_or("unknown");

            // Attempt to get timestamp from data["T"]; if missing, use top-level "ts" or "cts".
            let timestamp = data.get("T")
                .and_then(Value::as_i64)
                .or_else(|| value.get("ts").and_then(Value::as_i64))
                .or_else(|| value.get("cts").and_then(Value::as_i64))
                .unwrap_or_default()
                .max(0) as u64;

            // Log message type for debugging
            debug!("[Bybit] Orderbook message type: {}", message_type);

            // Use the symbol from the orderbook data.
            let original_symbol = data.get("s").and_then(Value::as_str).unwrap_or_default();
            let common_symbol = match symbol_mapper.map("Bybit", original_symbol) {
                Some(symbol) => symbol,
                None => {
                    // Use original symbol if no mapping exists
                    original_symbol.to_string()
                }
            };

            // Get precision from config
            let price_precision = config.feed_config.get_price_precision(&common_symbol);
            let quantity_precision = config.feed_config.get_quantity_precision(&common_symbol);

            // Process bid and ask arrays.
            // For delta messages: quantity=0 means delete the price level, otherwise update/add
            // HFT: Parse directly to scaled i64 (NO f64!)
            let bids: Vec<(i64, i64)> = data.get("b")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter().filter_map(|level| {
                        let price_str = level.get(0)?.as_str()?;
                        let qty_str = level.get(1)?.as_str()?;
                        let price = parse_to_scaled_or_default(price_str, price_precision);
                        let quantity = parse_to_scaled_or_default(qty_str, quantity_precision);

                        // For delta messages, quantity=0 means delete this price level
                        // We still include it in the data structure for downstream processing
                        // The consumer should handle quantity=0 as deletion
                        Some((price, quantity))
                    }).collect()
                })
                .unwrap_or_default();

            let asks: Vec<(i64, i64)> = data.get("a")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter().filter_map(|level| {
                        let price_str = level.get(0)?.as_str()?;
                        let qty_str = level.get(1)?.as_str()?;
                        let price = parse_to_scaled_or_default(price_str, price_precision);
                        let quantity = parse_to_scaled_or_default(qty_str, quantity_precision);

                        // For delta messages, quantity=0 means delete this price level
                        // We still include it in the data structure for downstream processing
                        Some((price, quantity))
                    }).collect()
                })
                .unwrap_or_default();

            // Handle snapshot vs delta internally for Bybit
            // For snapshot: full orderbook replacement
            // For delta: incremental updates (quantity=0 means delete)
            match message_type {
                "snapshot" => {
                    debug!("[Bybit] Processing snapshot for {}", original_symbol);
                    // For snapshots, use all data as-is
                },
                "delta" => {
                    debug!("[Bybit] Processing delta update for {} (bids: {}, asks: {})",
                        original_symbol, bids.len(), asks.len());
                    // For deltas, we still pass all data but log the distinction
                    // Downstream consumers can handle quantity=0 as deletion if needed
                },
                _ => {
                    debug!("[Bybit] Unknown message type '{}' for {}", message_type, original_symbol);
                }
            }

            let orderbook = crate::core::OrderBookData {
                exchange: "Bybit".to_string(),
                symbol: common_symbol,
                asset_type: asset_type.to_string(),
                bids,
                asks,
                price_precision,
                quantity_precision,
                timestamp,
                timestamp_unit: config.feed_config.timestamp_unit,
            };

            {
                let mut orderbooks = crate::core::ORDERBOOKS.write();
                orderbooks.push(orderbook.clone());
                // println!("[Bybit][{}] Orderbook stored. Now ORDERBOOKS count: {}", stream_type, orderbooks.len());
            }
            
            // Send UDP packet using multi-port sender if enabled, fallback to single-port
            debug!("Bybit: Receiving orderbook data for {}", orderbook.symbol);
            if USE_MULTI_PORT_UDP {
                if let Some(sender) = get_multi_port_sender() {
                    let _ = sender.send_orderbook_data(orderbook.clone());
                } else {
                    // Fallback to single-port
                    if let Some(sender) = crate::core::get_binary_udp_sender() {
                        let _ = sender.send_orderbook_data(orderbook.clone());
                    }
                }
            } else {
                // Use original single-port sender
                if let Some(sender) = crate::core::get_binary_udp_sender() {
                    let _ = sender.send_orderbook_data(orderbook.clone());
                }
            }
            debug!("Bybit: Sent UDP packet for {} orderbook with {} bids, {} asks",
                orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());
            
            crate::core::COMPARE_NOTIFY.notify_waiters();
        } else {
            debug!("[Bybit] Unhandled topic: {}", topic);
        }
    } else {
        debug!("[Bybit] No topic field found in message: {:?}", value);
    }
}


fn process_bybit_trade(trade_data: &Value, symbol_mapper: &Arc<SymbolMapper>, asset_type: &str, config: &ExchangeConfig) {
    let original_symbol = trade_data.get("s").and_then(Value::as_str).unwrap_or_default();
                let common_symbol = match symbol_mapper.map("Bybit", original_symbol) {
                Some(symbol) => symbol,
                None => {
                    // Use original symbol if no mapping exists
                    original_symbol.to_string()
                }
            };

    // Get precision from config
    let price_precision = config.feed_config.get_price_precision(&common_symbol);
    let quantity_precision = config.feed_config.get_quantity_precision(&common_symbol);

    // HFT: Parse directly to scaled i64 (NO f64!)
    let price = trade_data.get("p")
        .and_then(Value::as_str)
        .map(|s| parse_to_scaled_or_default(s, price_precision))
        .unwrap_or(0);
    let quantity = trade_data.get("v")
        .and_then(Value::as_str)
        .map(|s| parse_to_scaled_or_default(s, quantity_precision))
        .unwrap_or(0);
    let timestamp = trade_data.get("T")
        .and_then(Value::as_i64)
        .map(|t| {
            // Validate and normalize timestamp
            let t_u64 = t.max(0) as u64;
            let (is_valid, normalized) = crate::core::process_timestamp_field(t_u64, "Bybit");
            if !is_valid {
                // Invalid timestamp, use current time
                chrono::Utc::now().timestamp_millis() as u64
            } else {
                normalized
            }
        })
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);

    let trade = crate::core::TradeData {
        exchange: "Bybit".to_string(),
        symbol: common_symbol,
        asset_type: asset_type.to_string(),
        price,
        quantity,
        price_precision,
        quantity_precision,
        timestamp,
        timestamp_unit: config.feed_config.timestamp_unit,
    };

    {
        let mut trades = crate::core::TRADES.write();
        trades.push(trade.clone());
        // println!("[Bybit][{}] Trade stored. Now TRADES count: {}", stream_type, trades.len());
    }

    // Send UDP packet using multi-port sender if enabled, fallback to single-port
    if USE_MULTI_PORT_UDP {
        if let Some(sender) = get_multi_port_sender() {
            let _ = sender.send_trade_data(trade);
        } else {
            // Fallback to single-port
            if let Some(sender) = crate::core::get_binary_udp_sender() {
                let _ = sender.send_trade_data(trade);
            }
        }
    } else {
        // Use original single-port sender
        if let Some(sender) = crate::core::get_binary_udp_sender() {
            let _ = sender.send_trade_data(trade);
        }
    }
    
    crate::core::COMPARE_NOTIFY.notify_waiters();
}