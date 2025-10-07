
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

use crate::core::{OrderBookData, Feeder, SymbolMapper, CONNECTION_STATS, get_shutdown_receiver};
use crate::error::Result;
use crate::load_config::ExchangeConfig;

pub struct DeribitExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    ws_streams: HashMap<(String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    active_connections: Arc<AtomicUsize>,
    orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>,
}

impl DeribitExchange {
    pub fn new(config: ExchangeConfig, symbol_mapper: Arc<SymbolMapper>) -> Self {
        const SYMBOLS_PER_CONNECTION: usize = 5;
        let mut ws_streams = HashMap::new();
        
        let num_symbols = config.subscribe_data.codes.len();
        let num_chunks = (num_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;
        
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
            orderbooks: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Feeder for DeribitExchange {
    fn name(&self) -> &str {
        "Deribit"
    }

    async fn connect(&mut self) -> Result<()> {
        let config = self.config.clone();
        
        info!("Deribit: Creating {} WebSocket connections (5 symbols each, with both trade and orderbook streams)", self.ws_streams.len());
        
        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            let ws_url = "wss://www.deribit.com/ws/api/v2";
            debug!("Connecting to Deribit {} chunk {}: {}", asset_type, chunk_idx, ws_url);
            
            let mut retry_count = 0;
            let max_retries = 5;
            let mut retry_delay = Duration::from_secs(config.connect_config.initial_retry_delay_secs);
            
            loop {
                match connect_with_large_buffer(ws_url).await {
                    Ok((stream, _)) => {
                        *ws_stream = Some(stream);
                        info!("Successfully connected to Deribit {} chunk {} at {}", 
                            asset_type, chunk_idx, ws_url);
                        
                        // Update connection stats - mark as connected
                        {
                            let mut stats = CONNECTION_STATS.write();
                            let key = format!("Deribit_{}", asset_type);
                            let entry = stats.entry(key.clone()).or_default();
                            entry.connected += 1;
                            entry.total_connections += 1;
                            info!("[{}] Connection established for chunk {}", key, chunk_idx);
                        }
                        
                        debug!("Connected to Deribit {} chunk {}", asset_type, chunk_idx);
                        break;
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        retry_count += 1;
                        
                        warn!("Connection error for Deribit {} chunk {}: {}. Retrying in {} seconds (attempt {}/{})", 
                            asset_type, chunk_idx, error_str, retry_delay.as_secs(), retry_count, max_retries);
                        
                        if retry_count > max_retries {
                            warn!("Failed to connect to Deribit {} chunk {} after {} retries. Skipping this connection.", 
                                asset_type, chunk_idx, max_retries);
                            *ws_stream = None;
                            break;
                        }
                        
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(config.connect_config.max_retry_delay_secs));
                    }
                }
            }
            let connection_delay = Duration::from_millis(config.connect_config.connection_delay_ms.max(1000));
            tokio::time::sleep(connection_delay).await;
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<()> {
        const SYMBOLS_PER_CONNECTION: usize = 5;
        let config = self.config.clone();
        
        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            if let Some(ws_stream) = ws_stream {
                let start_idx = *chunk_idx * SYMBOLS_PER_CONNECTION;
                let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, config.subscribe_data.codes.len());
                let chunk_symbols = &config.subscribe_data.codes[start_idx..end_idx];
                
                let mut channels = Vec::new();
                for symbol in chunk_symbols {
                    channels.push(format!("trades.{}.100ms", symbol));
                    channels.push(format!("book.{}.100ms", symbol));
                }

                let subscribe_msg = json!({
                    "jsonrpc": "2.0",
                    "method": "public/subscribe",
                    "params": {
                        "channels": channels
                    }
                });

                let subscription = subscribe_msg.to_string();
                debug!("Sending subscription to Deribit {} chunk {} with {} symbols", 
                    asset_type, chunk_idx, chunk_symbols.len());
                
                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;
                info!("Sent subscription to Deribit {} chunk {} with channels: {:?}", 
                    asset_type, chunk_idx, channels);
                debug!("Subscribed to Deribit {} chunk {}", asset_type, chunk_idx);
            }
        }
        Ok(())
    }

        async fn start(&mut self) -> Result<()> {
        self.active_connections.store(0, Ordering::SeqCst);
        
        for ((asset_type, chunk_idx), ws_stream_opt) in self.ws_streams.iter_mut() {
            if let Some(mut ws_stream) = ws_stream_opt.take() {
                let asset_type = asset_type.clone();
                let chunk_idx = *chunk_idx;
                let symbol_mapper = self.symbol_mapper.clone();
                let active_connections = self.active_connections.clone();
                let orderbooks = self.orderbooks.clone();
                
                active_connections.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    debug!("Starting Deribit {} chunk {} stream processing (trade + orderbook)", 
                        asset_type, chunk_idx);
                    
                    let mut shutdown_rx = get_shutdown_receiver();
                    
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
                    
                    let mut last_message_time = std::time::Instant::now();
                    let mut health_check_interval = tokio::time::interval(Duration::from_secs(60));
                    
                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Deribit {} chunk {} received shutdown signal", asset_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Deribit {} chunk {}: {}", 
                                        asset_type, chunk_idx, e);
                                    error!("[Deribit_{}] WebSocket ping failed for chunk {}", asset_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to Deribit {} chunk {}", 
                                    asset_type, chunk_idx);
                            }
                            _ = health_check_interval.tick() => {
                                if last_message_time.elapsed() > Duration::from_secs(300) { // 5 minutes
                                    warn!("No messages received from Deribit {} chunk {} for {} seconds, connection may be stale", 
                                        asset_type, chunk_idx, last_message_time.elapsed().as_secs());
                                    
                                    if let Err(e) = ws_stream.send(Message::Ping(b"health_check".to_vec().into())).await {
                                        error!("Health check ping failed for Deribit {} chunk {}: {}", 
                                            asset_type, chunk_idx, e);
                                        break;
                                    }
                                }
                            }
                            msg = ws_stream.next().fuse() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        last_message_time = std::time::Instant::now();
                                        process_deribit_message(&text, symbol_mapper.clone(), &asset_type, orderbooks.clone());
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        last_message_time = std::time::Instant::now();
                                        debug!("Received pong from Deribit {} chunk {}", 
                                            asset_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Ping(data))) => {
                                        last_message_time = std::time::Instant::now();
                                        if let Err(e) = ws_stream.send(Message::Pong(data)).await {
                                            warn!("Failed to send pong to Deribit {} chunk {}: {}", 
                                                asset_type, chunk_idx, e);
                                            break;
                                        }
                                        debug!("Sent pong response to Deribit {} chunk {}", 
                                            asset_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("Deribit WebSocket closed for {} chunk {}: {:?}", 
                                            asset_type, chunk_idx, frame);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Deribit_{}", asset_type);
                                            let entry = stats.entry(key.clone()).or_default();
                                            if entry.connected > 0 {
                                                entry.connected -= 1;
                                                entry.disconnected += 1;
                                            }
                                            error!("[{}] WebSocket closed for chunk {}: {:?}", key, chunk_idx, frame);
                                        }
                                        break;
                                    },
                                    Some(Ok(msg)) => {
                                        debug!("Received other message type from Deribit: {:?}", msg);
                                    },
                                    Some(Err(e)) => {
                                        warn!("Deribit WebSocket error for {} chunk {}: {}", 
                                            asset_type, chunk_idx, e);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Deribit_{}", asset_type);
                                            let entry = stats.entry(key.clone()).or_default();
                                            if entry.connected > 0 {
                                                entry.connected -= 1;
                                                entry.disconnected += 1;
                                            }
                                            error!("[{}] WebSocket error for chunk {}: {}", key, chunk_idx, e);
                                        }
                                        break;
                                    },
                                    None => {
                                        warn!("WebSocket stream ended for Deribit {} chunk {}", 
                                            asset_type, chunk_idx);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Deribit_{}", asset_type);
                                            let entry = stats.entry(key.clone()).or_default();
                                            if entry.connected > 0 {
                                                entry.connected -= 1;
                                                entry.disconnected += 1;
                                            }
                                            error!("[{}] WebSocket stream ended for chunk {}", key, chunk_idx);
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    warn!("WebSocket connection lost for Deribit {} chunk {}, will be retried by feeder", 
                        asset_type, chunk_idx);
                    
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
        }

        while self.active_connections.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        warn!("All Deribit connections lost, returning to trigger reconnection");
        Ok(())
    }
}

fn process_deribit_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>) {
    if !text.contains("trades") && !text.contains("book") {
        debug!("{}", crate::core::safe_websocket_message_log("Deribit", text));
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", crate::core::safe_json_parse_error_log(&e, text));
            return;
        }
    };

    if let Some(method) = value.get("method").and_then(Value::as_str) {
        if method == "subscription" {
            if let Some(params) = value.get("params") {
                if let Some(channel) = params.get("channel").and_then(Value::as_str) {
                    if channel.starts_with("trades") {
                        if let Some(data) = params.get("data").and_then(Value::as_array) {
                            for trade_data in data {
                                let original_symbol = trade_data.get("instrument_name").and_then(Value::as_str).unwrap_or_default();
                                let common_symbol = match symbol_mapper.map("Deribit", original_symbol) {
                                    Some(symbol) => symbol,
                                    None => {
                                        // Use original symbol if no mapping exists
                                        original_symbol.to_string()
                                    }
                                };

                                let price = trade_data.get("price").and_then(Value::as_f64).unwrap_or_default();
                                let quantity = trade_data.get("amount").and_then(Value::as_f64).unwrap_or_default();
                                let timestamp = trade_data.get("timestamp").and_then(Value::as_i64).unwrap_or_default();

                                let trade = crate::core::TradeData {
                                    exchange: "Deribit".to_string(),
                                    symbol: common_symbol,
                                    asset_type: asset_type.to_string(),
                                    price,
                                    quantity,
                                    timestamp,
                                };

                                {
                                    let mut trades = crate::core::TRADES.write();
                                    trades.push(trade.clone());
                                }
                                
                                if let Some(sender) = crate::core::get_binary_udp_sender() {
                                    let _ = sender.send_trade_data(trade);
                                }
                                
                                crate::core::COMPARE_NOTIFY.notify_waiters();
                            }
                        }
                    } else if channel.starts_with("book") {
                        if let Some(data) = params.get("data") {
                            let original_symbol = data.get("instrument_name").and_then(Value::as_str).unwrap_or_default();
                            let common_symbol = match symbol_mapper.map("Deribit", original_symbol) {
                                Some(symbol) => symbol,
                                None => {
                                    // Use original symbol if no mapping exists
                                    original_symbol.to_string()
                                }
                            };

                            let timestamp = data.get("timestamp").and_then(Value::as_i64).unwrap_or_default();

                            let mut orderbooks = orderbooks.write();
                            let orderbook = orderbooks.entry(common_symbol.clone()).or_insert_with(|| crate::core::OrderBookData {
                                exchange: "Deribit".to_string(),
                                symbol: common_symbol.clone(),
                                asset_type: asset_type.to_string(),
                                bids: Vec::new(),
                                asks: Vec::new(),
                                timestamp: 0,
                timestamp_unit: crate::load_config::TimestampUnit::default(),
                            });

                            orderbook.timestamp = timestamp;

                            if let Some(bids) = data.get("bids").and_then(Value::as_array) {
                                for bid in bids {
                                    if let Some(bid_arr) = bid.as_array() {
                                        if bid_arr.len() == 3 {
                                            let change_type = bid_arr[0].as_str().unwrap_or_default();
                                            let price = bid_arr[1].as_f64().unwrap_or_default();
                                            let quantity = bid_arr[2].as_f64().unwrap_or_default();

                                            match change_type {
                                                "new" | "change" => {
                                                    if let Some(entry) = orderbook.bids.iter_mut().find(|(p, _)| *p == price) {
                                                        entry.1 = quantity;
                                                    } else {
                                                        orderbook.bids.push((price, quantity));
                                                    }
                                                }
                                                "delete" => {
                                                    orderbook.bids.retain(|(p, _)| *p != price);
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(asks) = data.get("asks").and_then(Value::as_array) {
                                for ask in asks {
                                    if let Some(ask_arr) = ask.as_array() {
                                        if ask_arr.len() == 3 {
                                            let change_type = ask_arr[0].as_str().unwrap_or_default();
                                            let price = ask_arr[1].as_f64().unwrap_or_default();
                                            let quantity = ask_arr[2].as_f64().unwrap_or_default();

                                            match change_type {
                                                "new" | "change" => {
                                                    if let Some(entry) = orderbook.asks.iter_mut().find(|(p, _)| *p == price) {
                                                        entry.1 = quantity;
                                                    } else {
                                                        orderbook.asks.push((price, quantity));
                                                    }
                                                }
                                                "delete" => {
                                                    orderbook.asks.retain(|(p, _)| *p != price);
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }

                            orderbook.bids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                            orderbook.asks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

                            if let Some(sender) = crate::core::get_binary_udp_sender() {
                                let _ = sender.send_orderbook_data(orderbook.clone());
                            }
                            
                            crate::core::COMPARE_NOTIFY.notify_waiters();
                        }
                    }
                }
            }
        }
    }
}
