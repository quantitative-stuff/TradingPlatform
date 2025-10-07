
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

use crate::core::{OrderBookData, Feeder, SymbolMapper, CONNECTION_STATS, get_shutdown_receiver, parse_to_scaled_or_default};
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

        // Combine futures and option symbols
        let total_symbols = config.subscribe_data.futures_symbols.len() + config.subscribe_data.option_symbols.len();
        let num_chunks = (total_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;

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
                // Combine futures and option symbols
                let mut all_symbols = config.subscribe_data.futures_symbols.clone();
                all_symbols.extend(config.subscribe_data.option_symbols.clone());

                let start_idx = *chunk_idx * SYMBOLS_PER_CONNECTION;
                let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, all_symbols.len());
                let chunk_symbols = &all_symbols[start_idx..end_idx];

                let mut channels = Vec::new();
                for symbol in chunk_symbols {
                    // Subscribe to ticker, trades and orderbook for each instrument
                    channels.push(format!("ticker.{}.100ms", symbol));
                    channels.push(format!("trades.{}.100ms", symbol));
                    channels.push(format!("book.{}.100ms", symbol));
                }

                // Use unique ID for each subscription message (based on chunk_idx)
                let subscribe_msg = json!({
                    "jsonrpc": "2.0",
                    "id": 8066 + chunk_idx,
                    "method": "public/subscribe",
                    "params": {
                        "channels": channels
                    }
                });

                let subscription = subscribe_msg.to_string();
                info!("[DERIBIT SUBSCRIBE] Sending subscription to {} chunk {} with {} symbols. Full message: {}",
                    asset_type, chunk_idx, chunk_symbols.len(), subscription);

                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;
                info!("[DERIBIT SUBSCRIBE] Sent subscription to {} chunk {} with channels: {:?}", 
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
                let config = self.config.clone();

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
                                        process_deribit_message(&text, symbol_mapper.clone(), &asset_type, orderbooks.clone(), &config);
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

fn process_deribit_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>, config: &ExchangeConfig) {
    // Log all messages for debugging
    info!("[DERIBIT DEBUG] Received message: {}", text);

    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            error!("[DERIBIT ERROR] Failed to parse JSON: {} - Raw: {}", e, text);
            return;
        }
    };

    // Check for subscription result or error
    if let Some(id) = value.get("id") {
        if let Some(result) = value.get("result") {
            info!("[DERIBIT RESPONSE] Subscription successful for id {}: {:?}", id, result);
        } else if let Some(error) = value.get("error") {
            error!("[DERIBIT ERROR] Subscription failed for id {}: {:?}", id, error);
        }
    }

    if let Some(method) = value.get("method").and_then(Value::as_str) {
        info!("[DERIBIT METHOD] Received method: {}", method);
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

                                // HFT: Get precision from config
                                let price_precision = config.feed_config.get_price_precision(&common_symbol);
                                let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                                // HFT: Parse directly to scaled i64 (NO f64!)
                                // Deribit sends as JSON number (f64), so convert: (value * 10^precision) as i64
                                let price = trade_data.get("price").and_then(Value::as_f64)
                                    .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)
                                    .unwrap_or(0);
                                let quantity = trade_data.get("amount").and_then(Value::as_f64)
                                    .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)
                                    .unwrap_or(0);
                                let timestamp = trade_data.get("timestamp").and_then(Value::as_i64).unwrap_or_default().max(0) as u64;

                                let trade = crate::core::TradeData {
                                    exchange: "Deribit".to_string(),
                                    symbol: common_symbol,
                                    asset_type: asset_type.to_string(),
                                    price,
                                    quantity,
                                    price_precision,
                                    quantity_precision: qty_precision,
                                    timestamp,
                                    timestamp_unit: crate::load_config::TimestampUnit::Milliseconds,
                                };

                                {
                                    let mut trades = crate::core::TRADES.write();
                                    trades.push(trade.clone());
                                }
                                
                                if let Some(sender) = crate::core::get_multi_port_sender() {
                                    let _ = sender.send_trade_data(trade);
                                }
                                
                                crate::core::COMPARE_NOTIFY.notify_waiters();
                            }
                        }
                    } else if channel.starts_with("ticker") {
                        // Handle ticker messages
                        if let Some(data) = params.get("data") {
                            let original_symbol = data.get("instrument_name").and_then(Value::as_str).unwrap_or_default();
                            let common_symbol = match symbol_mapper.map("Deribit", original_symbol) {
                                Some(symbol) => symbol,
                                None => {
                                    // Use original symbol if no mapping exists
                                    original_symbol.to_string()
                                }
                            };

                            // HFT: Get precision from config
                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            // Extract ticker data - last price, best bid/ask
                            if let Some(last_price) = data.get("last_price").and_then(Value::as_f64) {
                                let timestamp = data.get("timestamp").and_then(Value::as_i64).unwrap_or_default().max(0) as u64;
                                let quantity = data.get("stats").and_then(|s| s.get("volume")).and_then(Value::as_f64)
                                    .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)
                                    .unwrap_or(0);
                                let price = (last_price * 10_f64.powi(price_precision as i32)) as i64;

                                // Create a trade from ticker's last price
                                let trade = crate::core::TradeData {
                                    exchange: "Deribit".to_string(),
                                    symbol: common_symbol.clone(),
                                    asset_type: asset_type.to_string(),
                                    price,
                                    quantity,
                                    price_precision,
                                    quantity_precision: qty_precision,
                                    timestamp,
                                    timestamp_unit: crate::load_config::TimestampUnit::Milliseconds,
                                };

                                {
                                    let mut trades = crate::core::TRADES.write();
                                    trades.push(trade.clone());
                                }

                                if let Some(sender) = crate::core::get_multi_port_sender() {
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

                            // HFT: Get precision BEFORE parsing orderbook
                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            let timestamp = data.get("timestamp").and_then(Value::as_i64).unwrap_or_default().max(0) as u64;

                            let mut orderbooks = orderbooks.write();
                            let orderbook = orderbooks.entry(common_symbol.clone()).or_insert_with(|| crate::core::OrderBookData {
                                exchange: "Deribit".to_string(),
                                symbol: common_symbol.clone(),
                                asset_type: asset_type.to_string(),
                                bids: Vec::new(),
                                asks: Vec::new(),
                                price_precision,
                                quantity_precision: qty_precision,
                                timestamp: 0,
                                timestamp_unit: crate::load_config::TimestampUnit::default(),
                            });

                            orderbook.timestamp = timestamp;

                            if let Some(bids) = data.get("bids").and_then(Value::as_array) {
                                for bid in bids {
                                    if let Some(bid_arr) = bid.as_array() {
                                        if bid_arr.len() == 3 {
                                            let change_type = bid_arr[0].as_str().unwrap_or_default();
                                            let price = bid_arr[1].as_f64()
                                                .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)
                                                .unwrap_or(0);
                                            let quantity = bid_arr[2].as_f64()
                                                .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)
                                                .unwrap_or(0);

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
                                            let price = ask_arr[1].as_f64()
                                                .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)
                                                .unwrap_or(0);
                                            let quantity = ask_arr[2].as_f64()
                                                .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)
                                                .unwrap_or(0);

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

                            orderbook.bids.sort_by(|a, b| b.0.cmp(&a.0));
                            orderbook.asks.sort_by(|a, b| a.0.cmp(&b.0));

                            if let Some(sender) = crate::core::get_multi_port_sender() {
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
