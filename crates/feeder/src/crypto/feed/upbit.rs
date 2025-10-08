use async_trait::async_trait;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use super::websocket_config::connect_with_large_buffer;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn, error};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::{Feeder, TRADES, ORDERBOOKS, SymbolMapper, COMPARE_NOTIFY, get_shutdown_receiver, TradeData, OrderBookData, CONNECTION_STATS, parse_to_scaled_or_default};
use crate::error::{Result, Error};
use crate::load_config::ExchangeConfig;

pub struct UpbitExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    // Map of (asset_type, chunk_index) -> WebSocket connection
    // Each connection handles up to 5 symbols with both trade and orderbook streams
    ws_streams: HashMap<(String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    // Track active connections for proper reconnection
    active_connections: Arc<AtomicUsize>,
}

impl UpbitExchange {
    pub fn new(
        config: ExchangeConfig,
        symbol_mapper: Arc<SymbolMapper>,
    ) -> Self {
        let limits = crate::core::robust_connection::ExchangeConnectionLimits::for_exchange("upbit");
        let symbols_per_connection = limits.max_symbols_per_connection;
        let mut ws_streams = HashMap::new();
        
        // Calculate how many connections we need based on fault tolerance (20 symbols per connection)
        let num_symbols = config.subscribe_data.spot_symbols.len();
        let num_chunks = (num_symbols + symbols_per_connection - 1) / symbols_per_connection;
        
        debug!("Upbit constructor - {} symbols, {} per connection = {} connections",
                 num_symbols, symbols_per_connection, num_chunks);
        
        // Initialize connections for each chunk of symbols
        // Now each connection will handle BOTH trade and orderbook for its symbols
        for asset_type in &config.feed_config.asset_type {
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

    async fn connect_websocket(&mut self, asset_type: &str, chunk_idx: usize) -> Result<()> {
        let limits = crate::core::robust_connection::ExchangeConnectionLimits::for_exchange("upbit");
        let symbols_per_connection = limits.max_symbols_per_connection;
        
        // Get the symbols for this chunk (20 symbols per connection for fault tolerance)
        let start_idx = chunk_idx * symbols_per_connection;
        let end_idx = std::cmp::min(start_idx + symbols_per_connection, self.config.subscribe_data.spot_symbols.len());
        let chunk_symbols = &self.config.subscribe_data.spot_symbols[start_idx..end_idx];
        
        debug!("Attempting to connect to Upbit for {} chunk {}", asset_type, chunk_idx);
        debug!("  Symbols in this connection: {:?}", chunk_symbols);
        info!("Upbit {}-{}: Handling {} symbols ({})", 
            asset_type, chunk_idx, chunk_symbols.len(),
            chunk_symbols.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(", ") + 
            if chunk_symbols.len() > 3 { "..." } else { "" });
        
        let ws_url = &self.config.connect_config.ws_url;
        
        match connect_with_large_buffer(ws_url).await {
            Ok((ws_stream, response)) => {
                debug!("Upbit WebSocket connected for {} chunk {}", asset_type, chunk_idx);
                debug!("Upbit Connection response: {:?}", response);
                
                // Update connection stats - mark as connected
                {
                    let mut stats = CONNECTION_STATS.write();
                    let key = format!("Upbit_{}", asset_type);
                    let entry = stats.entry(key.clone()).or_default();
                    entry.connected += 1;
                    entry.total_connections += 1;
                    info!("[{}] Connection established for chunk {}", key, chunk_idx);
                }
                
                // Log successful connection to QuestDB
                let connection_id = format!("upbit-{}-{}", asset_type, chunk_idx);
                let _ = crate::core::feeder_metrics::log_connection(
                    "Upbit", 
                    &connection_id, 
                    crate::connect_to_databse::ConnectionEvent::Connected
                ).await;
                
                if let Some(stream_slot) = self.ws_streams.get_mut(&(asset_type.to_string(), chunk_idx)) {
                    *stream_slot = Some(ws_stream);
                }
                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to Upbit WebSocket for {} chunk {}: {}", 
                    asset_type, chunk_idx, e);
                Err(Error::Connection(format!("Upbit connection failed: {}", e)))
            }
        }
    }
}

#[async_trait]
impl Feeder for UpbitExchange {
    fn name(&self) -> &str {
        "Upbit"
    }

    async fn connect(&mut self) -> Result<()> {
        let mut retry_delay = Duration::from_secs(self.config.connect_config.initial_retry_delay_secs);
        let connection_keys: Vec<(String, usize)> = self.ws_streams
            .keys()
            .map(|(asset, chunk)| (asset.clone(), *chunk))
            .collect();
        
        info!("Upbit: Creating {} WebSocket connections (5 symbols each, with both trade and orderbook streams)", connection_keys.len());
        
        for (asset_type, chunk_idx) in connection_keys {
            loop {
                match self.connect_websocket(&asset_type, chunk_idx).await {
                    Ok(()) => {
                        debug!("Successfully connected to Upbit {} chunk {}", 
                            asset_type, chunk_idx);
                        retry_delay = Duration::from_secs(self.config.connect_config.initial_retry_delay_secs);  // Reset delay on success
                        break;  // Move to next connection
                    },
                    Err(e) => {
                        error!("Failed to connect to Upbit {} chunk {}: {}", 
                            asset_type, chunk_idx, e);
                        debug!("Retrying in {} seconds...", retry_delay.as_secs());
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(self.config.connect_config.max_retry_delay_secs));
                        // Continue trying this connection
                    }
                }
            }
            // Upbit-specific connection delay: 5 connections per second limit
            // Use 1 second delay between connections to stay well under limit
            tokio::time::sleep(Duration::from_millis(1200)).await;
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<()> {
        let limits = crate::core::robust_connection::ExchangeConnectionLimits::for_exchange("upbit");
        let symbols_per_connection = limits.max_symbols_per_connection;
        
        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            if let Some(ws_stream) = ws_stream {
                // Get the symbols for this chunk (20 symbols per connection)
                let start_idx = *chunk_idx * symbols_per_connection;
                let end_idx = std::cmp::min(start_idx + symbols_per_connection, self.config.subscribe_data.spot_symbols.len());
                let chunk_symbols = &self.config.subscribe_data.spot_symbols[start_idx..end_idx];

                // Create Upbit subscription format (order matters!)
                let mut subscription_array = vec![
                    json!({ "ticket": format!("upbit-{}-{}", asset_type, chunk_idx) })
                ];

                // Add subscriptions based on stream_type configuration
                for stream_type in &self.config.subscribe_data.stream_type {
                    match stream_type.as_str() {
                        "trade" => {
                            subscription_array.push(json!({
                                "type": "trade",
                                "codes": chunk_symbols,
                                "isOnlyRealtime": true
                            }));
                        },
                        "orderbook" => {
                            if self.config.subscribe_data.order_depth > 0 {
                                subscription_array.push(json!({
                                    "type": "orderbook",
                                    "codes": chunk_symbols,
                                    "isOnlyRealtime": true
                                }));
                            }
                        },
                        _ => {
                            warn!("Unknown stream type for Upbit: {}", stream_type);
                        }
                    }
                }

                // Add format at the end
                subscription_array.push(json!({ "format": "DEFAULT" }));

                let subscription = serde_json::to_string(&subscription_array)?;
                info!("Upbit subscription message: {}", subscription);
                debug!("Sending subscription to Upbit {} chunk {} with {} symbols",
                    asset_type, chunk_idx, chunk_symbols.len());

                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;
                info!("Sent subscription to Upbit {} chunk {} with symbols: {:?}",
                    asset_type, chunk_idx, chunk_symbols);
                
                // Log successful subscription with symbol details to QuestDB
                let connection_id = format!("upbit-{}-{}", asset_type, chunk_idx);
                let _ = crate::core::feeder_metrics::log_connection(
                    "Upbit", 
                    &connection_id, 
                    crate::connect_to_databse::ConnectionEvent::SubscriptionSuccess { 
                        symbols: chunk_symbols.iter().map(|s| s.to_string()).collect()
                    }
                ).await;
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
                    debug!("Starting Upbit {} chunk {} stream processing (trade + orderbook)", 
                        asset_type, chunk_idx);
                    
                    // Get shutdown receiver
                    let mut shutdown_rx = get_shutdown_receiver();
                    
                    // Add periodic ping to keep connection alive
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
                    
                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Upbit {} chunk {} received shutdown signal", asset_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                // Send ping to keep connection alive
                                if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Upbit {} chunk {}: {}", 
                                        asset_type, chunk_idx, e);
                                    error!("Connection lost for Upbit {} chunk {}", 
                                        asset_type, chunk_idx);
                                    
                                    // Log disconnection to QuestDB
                                    let connection_id = format!("upbit-{}-{}", asset_type, chunk_idx);
                                    let _ = crate::core::feeder_metrics::log_connection(
                                        "Upbit", 
                                        &connection_id, 
                                        crate::connect_to_databse::ConnectionEvent::Disconnected { 
                                            reason: "ping failed".to_string() 
                                        }
                                    ).await;
                                    error!("[Upbit_{}] WebSocket disconnected for chunk {} - ping failed", asset_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to Upbit {} chunk {}", 
                                    asset_type, chunk_idx);
                            }
                            msg = ws_stream.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        process_upbit_message(&text, symbol_mapper.clone(), &asset_type, &config);
                                    },
                                    Some(Ok(Message::Binary(data))) => {
                                        // Upbit sends binary data, decode it
                                        if let Ok(text) = String::from_utf8(data.to_vec()) {
                                            process_upbit_message(&text, symbol_mapper.clone(), &asset_type, &config);
                                        } else {
                                            debug!("Failed to decode binary message from Upbit {} chunk {}", asset_type, chunk_idx);
                                        }
                                    },
                                    Some(Ok(Message::Ping(ping))) => {
                                        if let Err(e) = ws_stream.send(Message::Pong(ping)).await {
                                            debug!("Failed to send pong: {}", e);
                                            break;
                                        }
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        debug!("Received pong from Upbit {} chunk {}", 
                                            asset_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("WebSocket closed by Upbit server: {:?}", frame);
                                        error!("[Upbit_{}] WebSocket issue for chunk {}", asset_type, chunk_idx);
                                        break;
                                    },
                                    Some(Ok(_)) => {
                                        // Handle other message types if needed
                                    },
                                    Some(Err(e)) => {
                                        let error_str = e.to_string();
                                        if error_str.contains("10054") || error_str.contains("forcibly closed") || error_str.contains("강제로 끊겼습니다") {
                                            warn!("Upbit {} chunk {} connection forcibly closed by remote host (error 10054)", asset_type, chunk_idx);
                                        } else {
                                            error!("WebSocket error for Upbit {} chunk {}: {}", 
                                                asset_type, chunk_idx, e);
                                        }
                                        error!("[Upbit_{}] WebSocket issue for chunk {}", asset_type, chunk_idx);
                                        break;
                                    },
                                    None => {
                                        warn!("WebSocket stream ended for Upbit {} chunk {}", 
                                            asset_type, chunk_idx);
                                        error!("[Upbit_{}] WebSocket issue for chunk {}", asset_type, chunk_idx);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    warn!("Upbit {} chunk {} disconnected. Connection will be retried by feeder.", 
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

        warn!("All Upbit connections lost, returning to trigger reconnection");
        Ok(())
    }
}

fn process_upbit_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, config: &ExchangeConfig) {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", crate::core::safe_json_parse_error_log(&e, text));
            return;
        }
    };

    // Determine message type from Upbit data structure
    let message_type = value.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if message_type.is_empty() {
        warn!("Upbit message has no type field. Available fields: {:?}", value.as_object().map(|obj| obj.keys().collect::<Vec<_>>()));
        warn!("Upbit message has no type field: {}", text);
        return;
    }

    match message_type {
        "trade" => {
            // Get the original symbol from Upbit trade message
            let original_symbol = value.get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if original_symbol.is_empty() {
                return;
            }

            let common_symbol = match symbol_mapper.map("Upbit", original_symbol) {
                Some(symbol) => symbol,
                None => return,
            };

            // HFT: Get precision from config
            let price_precision = config.feed_config.get_price_precision(&common_symbol);
            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

            // HFT: Parse directly to scaled i64 (NO f64!)
            // Upbit sends as JSON number (f64), so convert: (value * 10^precision) as i64
            let price = value["trade_price"].as_f64()
                .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)
                .unwrap_or(0);
            let quantity = value["trade_volume"].as_f64()
                .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)
                .unwrap_or(0);
            let timestamp = value["trade_timestamp"].as_i64().unwrap_or_default().max(0) as u64;

            debug!("TRADE {} @ {} x {}", common_symbol, price, quantity);

            let trade = TradeData {
                exchange: "Upbit".to_string(),
                symbol: common_symbol,
                asset_type: asset_type.to_string(),
                price,
                quantity,
                price_precision,
                quantity_precision: qty_precision,
                timestamp,
                timestamp_unit: config.feed_config.timestamp_unit,
            };

            {
                let mut trades = TRADES.write();
                trades.push(trade.clone());
            }

            // Send UDP packet immediately using binary sender (90% smaller packets!)
            if let Some(sender) = crate::core::get_multi_port_sender() {
                let _ = sender.send_trade_data(trade);
            }
            
            COMPARE_NOTIFY.notify_waiters();
        },
        "orderbook" => {
            // Get symbol from orderbook message
            let original_symbol = value.get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if original_symbol.is_empty() {
                debug!("Cannot determine symbol for Upbit orderbook message");
                return;
            }

            let common_symbol = match symbol_mapper.map("Upbit", original_symbol) {
                Some(symbol) => symbol,
                None => {
                    // Silent - this is expected for some symbols
                    return;
                }
            };

            // HFT: Get precision BEFORE parsing orderbook
            let price_precision = config.feed_config.get_price_precision(&common_symbol);
            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

            // Extract orderbook data
            let orderbook_units = value.get("orderbook_units")
                .and_then(|v| v.as_array());

            if let Some(units) = orderbook_units {
                let orderbook = OrderBookData {
                    exchange: "Upbit".to_string(),
                    symbol: common_symbol.clone(),
                    asset_type: asset_type.to_string(),
                    bids: units.iter()
                        .filter_map(|unit| {
                            let price = unit["bid_price"].as_f64()
                                .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)?;
                            let size = unit["bid_size"].as_f64()
                                .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)?;
                            Some((price, size))
                        })
                        .collect(),
                    asks: units.iter()
                        .filter_map(|unit| {
                            let price = unit["ask_price"].as_f64()
                                .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)?;
                            let size = unit["ask_size"].as_f64()
                                .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)?;
                            Some((price, size))
                        })
                        .collect(),
                    price_precision,
                    quantity_precision: qty_precision,
                    timestamp: value.get("timestamp")
                        .and_then(|v| v.as_i64())
                        .map(|t| t.max(0) as u64)
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64),
                    timestamp_unit: config.feed_config.timestamp_unit,
                };

                {
                    let mut orderbooks = ORDERBOOKS.write();
                    orderbooks.push(orderbook.clone());
                }

                // Send UDP packet immediately using binary sender (90% smaller packets!)
                if let Some(sender) = crate::core::get_multi_port_sender() {
                    let _ = sender.send_orderbook_data(orderbook);
                }
                
                COMPARE_NOTIFY.notify_waiters();
            }
        },
        _ => {
            debug!("Received unknown Upbit message type: {}", message_type);
        }
    }
}