
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

pub struct BithumbExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    // Key: (asset_type, stream_type, chunk_idx) where stream_type is "trade" or "orderbook"
    ws_streams: HashMap<(String, String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    active_connections: Arc<AtomicUsize>,
    orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>,
}

impl BithumbExchange {
    pub fn new(config: ExchangeConfig, symbol_mapper: Arc<SymbolMapper>) -> Self {
        const SYMBOLS_PER_CONNECTION: usize = 10;
        let mut ws_streams = HashMap::new();

        let num_symbols = config.subscribe_data.spot_symbols.len();
        let num_chunks = (num_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;

        // Create separate connections for trade and orderbook streams
        for asset_type in &config.feed_config.asset_type {
            for stream_type in &["trade", "orderbook"] {
                for chunk_idx in 0..num_chunks {
                    ws_streams.insert(
                        (asset_type.clone(), stream_type.to_string(), chunk_idx),
                        None
                    );
                }
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
impl Feeder for BithumbExchange {
    fn name(&self) -> &str {
        "Bithumb"
    }

    async fn connect(&mut self) -> Result<()> {
        let config = self.config.clone();

        info!("Bithumb: Creating {} WebSocket connections (10 symbols each, separate for trade and orderbook)", self.ws_streams.len());

        for ((asset_type, stream_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            let ws_url = "wss://ws-api.bithumb.com/websocket/v1";
            debug!("Connecting to Bithumb {} {} chunk {}: {}", asset_type, stream_type, chunk_idx, ws_url);

            let mut retry_count = 0;
            let max_retries = 5;
            let mut retry_delay = Duration::from_secs(config.connect_config.initial_retry_delay_secs);

            loop {
                match connect_with_large_buffer(ws_url).await {
                    Ok((stream, _)) => {
                        *ws_stream = Some(stream);
                        info!("Successfully connected to Bithumb {} {} chunk {} at {}",
                            asset_type, stream_type, chunk_idx, ws_url);

                        // Update connection stats - mark as connected
                        {
                            let mut stats = CONNECTION_STATS.write();
                            let key = format!("Bithumb_{}_{}", asset_type, stream_type);
                            let entry = stats.entry(key.clone()).or_default();
                            entry.connected += 1;
                            entry.total_connections += 1;
                            info!("[{}] Connection established for chunk {}", key, chunk_idx);
                        }

                        debug!("Connected to Bithumb {} {} chunk {}", asset_type, stream_type, chunk_idx);
                        break;
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        retry_count += 1;

                        warn!("Connection error for Bithumb {} {} chunk {}: {}. Retrying in {} seconds (attempt {}/{})",
                            asset_type, stream_type, chunk_idx, error_str, retry_delay.as_secs(), retry_count, max_retries);

                        if retry_count > max_retries {
                            warn!("Failed to connect to Bithumb {} {} chunk {} after {} retries. Skipping this connection.",
                                asset_type, stream_type, chunk_idx, max_retries);
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
        const SYMBOLS_PER_CONNECTION: usize = 10;
        let config = self.config.clone();

        for ((asset_type, stream_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            if let Some(ws_stream) = ws_stream {
                let start_idx = *chunk_idx * SYMBOLS_PER_CONNECTION;
                let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, config.subscribe_data.spot_symbols.len());
                let chunk_symbols = &config.subscribe_data.spot_symbols[start_idx..end_idx];

                // Symbols are already in KRW-BTC format from config
                let codes: Vec<String> = chunk_symbols.iter()
                    .map(|s| s.to_string())
                    .collect();

                // Send ONLY the subscription for this stream type
                let subscription_msg = if stream_type == "trade" {
                    json!([
                        {"ticket": format!("bithumb_trade_{}_{}", asset_type, chunk_idx)},
                        {"type": "trade", "codes": codes},
                        {"format": "DEFAULT"}
                    ])
                } else {
                    json!([
                        {"ticket": format!("bithumb_orderbook_{}_{}", asset_type, chunk_idx)},
                        {"type": "orderbook", "codes": codes, "level": 10},
                        {"format": "DEFAULT"}
                    ])
                };

                let subscription = subscription_msg.to_string();
                debug!("Sending Bithumb {} subscription for {} chunk {} with {} symbols",
                    stream_type, asset_type, chunk_idx, chunk_symbols.len());
                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;

                info!("Sent {} subscription to Bithumb {} chunk {} with symbols: {:?}",
                    stream_type, asset_type, chunk_idx, chunk_symbols);
            }
        }
        Ok(())
    }

        async fn start(&mut self) -> Result<()> {
        self.active_connections.store(0, Ordering::SeqCst);

        for ((asset_type, stream_type, chunk_idx), ws_stream_opt) in self.ws_streams.iter_mut() {
            if let Some(mut ws_stream) = ws_stream_opt.take() {
                let asset_type = asset_type.clone();
                let stream_type = stream_type.clone();
                let chunk_idx = *chunk_idx;
                let symbol_mapper = self.symbol_mapper.clone();
                let active_connections = self.active_connections.clone();
                let orderbooks = self.orderbooks.clone();
                let config = self.config.clone();

                active_connections.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    info!("Starting Bithumb {} {} chunk {} stream processing", asset_type, stream_type, chunk_idx);

                    let mut shutdown_rx = get_shutdown_receiver();

                    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));

                    let mut last_message_time = std::time::Instant::now();
                    let mut health_check_interval = tokio::time::interval(Duration::from_secs(60));

                    debug!("Bithumb {} {} chunk {} waiting for messages...", asset_type, stream_type, chunk_idx);

                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Bithumb {} {} chunk {} received shutdown signal", asset_type, stream_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Bithumb {} {} chunk {}: {}",
                                        asset_type, stream_type, chunk_idx, e);
                                    error!("[Bithumb_{}_{}] WebSocket ping failed for chunk {}", asset_type, stream_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to Bithumb {} {} chunk {}",
                                    asset_type, stream_type, chunk_idx);
                            }
                            _ = health_check_interval.tick() => {
                                if last_message_time.elapsed() > Duration::from_secs(300) { // 5 minutes
                                    warn!("No messages received from Bithumb {} {} chunk {} for {} seconds, connection may be stale",
                                        asset_type, stream_type, chunk_idx, last_message_time.elapsed().as_secs());

                                    if let Err(e) = ws_stream.send(Message::Ping(b"health_check".to_vec().into())).await {
                                        error!("Health check ping failed for Bithumb {} {} chunk {}: {}",
                                            asset_type, stream_type, chunk_idx, e);
                                        break;
                                    }
                                }
                            }
                            msg = ws_stream.next().fuse() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        debug!("Bithumb {} {} chunk {} received text message", asset_type, stream_type, chunk_idx);
                                        last_message_time = std::time::Instant::now();
                                        process_bithumb_message(&text, symbol_mapper.clone(), &asset_type, orderbooks.clone(), &config);
                                    },
                                    Some(Ok(Message::Binary(data))) => {
                                        // Bithumb sends binary data, decode it as UTF-8
                                        if let Ok(text) = String::from_utf8(data.to_vec()) {
                                            // Check for error messages
                                            if text.contains("error") && data.len() < 200 {
                                                warn!("Bithumb {} {} chunk {} error: {}",
                                                    asset_type, stream_type, chunk_idx, text);
                                            }
                                            last_message_time = std::time::Instant::now();
                                            process_bithumb_message(&text, symbol_mapper.clone(), &asset_type, orderbooks.clone(), &config);
                                        } else {
                                            debug!("Bithumb {} {} chunk {} received binary message ({} bytes) - failed to decode as UTF-8",
                                                asset_type, stream_type, chunk_idx, data.len());
                                            // Show raw bytes for debugging
                                            if data.len() < 100 {
                                                debug!("  Raw bytes: {:?}", &data[..]);
                                            }
                                            last_message_time = std::time::Instant::now();
                                        }
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        // Pong received - connection is alive
                                        last_message_time = std::time::Instant::now();
                                    },
                                    Some(Ok(Message::Ping(data))) => {
                                        // Ping received, sending pong
                                        last_message_time = std::time::Instant::now();
                                        if let Err(e) = ws_stream.send(Message::Pong(data)).await {
                                            warn!("Failed to send pong to Bithumb {} {} chunk {}: {}",
                                                asset_type, stream_type, chunk_idx, e);
                                            break;
                                        }
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("Bithumb WebSocket closed for {} {} chunk {}: {:?}",
                                            asset_type, stream_type, chunk_idx, frame);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Bithumb_{}_{}", asset_type, stream_type);
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
                                        debug!("Received other message type from Bithumb: {:?}", msg);
                                    },
                                    Some(Err(e)) => {
                                        warn!("Bithumb WebSocket error for {} {} chunk {}: {}",
                                            asset_type, stream_type, chunk_idx, e);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Bithumb_{}_{}", asset_type, stream_type);
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
                                        warn!("WebSocket stream ended for Bithumb {} {} chunk {}",
                                            asset_type, stream_type, chunk_idx);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Bithumb_{}_{}", asset_type, stream_type);
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
                    warn!("WebSocket connection lost for Bithumb {} {} chunk {}, will be retried by feeder",
                        asset_type, stream_type, chunk_idx);
                    
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
        }

        while self.active_connections.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        warn!("All Bithumb connections lost, returning to trigger reconnection");
        Ok(())
    }
}

fn process_bithumb_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>, config: &ExchangeConfig) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static MESSAGE_COUNT: AtomicU64 = AtomicU64::new(0);

    let count = MESSAGE_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 100 == 0 {
        debug!("Bithumb: Processed {} messages so far", count);
    }

    // Always show first few messages to debug
    if count < 10 {
        debug!("Bithumb raw message #{}: {}", count, text);
    }

    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            warn!("Bithumb JSON parse error: {} - Message: {}", e, text);
            return;
        }
    };

    // Check for status messages
    if let Some(status) = value.get("status").and_then(Value::as_str) {
        if status == "0000" {
            info!("Bithumb subscription successful - status: {} - Message: {}", status, text);
        } else {
            warn!("Bithumb status: {} - Full message: {}", status, text);
        }
        return;
    }

    // Check for resmsg (response message)
    if let Some(resmsg) = value.get("resmsg").and_then(Value::as_str) {
        info!("Bithumb response: {} - Full message: {}", resmsg, text);
        return;
    }

    // Check the message type field
    let msg_type = value.get("type")
        .and_then(Value::as_str)
        .unwrap_or("");

    if !msg_type.is_empty() {
        info!("Bithumb message type: {} (total: {})", msg_type, count);

        match msg_type {
            "trade" => {
                // Trade format from Bithumb - symbol can be in either "code" or "cd" field
                let original_symbol = value.get("code")
                    .or_else(|| value.get("cd"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                // No conversion needed - use symbol directly
                let common_symbol = match symbol_mapper.map("Bithumb", original_symbol) {
                    Some(symbol) => symbol,
                    None => {
                        if count % 100 == 0 {
                            debug!("[Bithumb] No mapping found for trade symbol: {}",
                                original_symbol);
                        }
                        return; // Skip this message if no mapping found
                    }
                };

                // HFT: Get precision from config
                let price_precision = config.feed_config.get_price_precision(&common_symbol);
                let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                // HFT: Parse directly to scaled i64 (NO f64!)
                // Bithumb sends as JSON number (f64), so convert: (value * 10^precision) as i64
                let price = value.get("trade_price")
                    .or_else(|| value.get("tp"))
                    .and_then(Value::as_f64)
                    .map(|v| (v * 10_f64.powi(price_precision as i32)) as i64)
                    .unwrap_or(0);
                let quantity = value.get("trade_volume")
                    .or_else(|| value.get("tv"))
                    .and_then(Value::as_f64)
                    .map(|v| (v * 10_f64.powi(qty_precision as i32)) as i64)
                    .unwrap_or(10_i64.pow(qty_precision as u32)); // Default to 1.0 scaled
                let timestamp = value.get("trade_timestamp")
                    .or_else(|| value.get("ttms"))
                    .or_else(|| value.get("timestamp"))
                    .and_then(Value::as_i64)
                    .map(|t| t.max(0) as u64)
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);

                if count % 50 == 0 {
                    debug!("Bithumb TRADE {} @ {} x {}", common_symbol, price, quantity);
                }

                let trade = crate::core::TradeData {
                    exchange: "Bithumb".to_string(),
                    symbol: common_symbol.clone(),
                    asset_type: asset_type.to_string(),
                    price,
                    quantity,
                    price_precision,
                    quantity_precision: qty_precision,
                    timestamp,
                    timestamp_unit: config.feed_config.timestamp_unit,
                };

                {
                    let mut trades = crate::core::TRADES.write();
                    trades.push(trade.clone());
                }

                if let Some(sender) = crate::core::get_multi_port_sender() {
                    let _ = sender.send_trade_data(trade.clone());
                    if count % 100 == 0 {
                        debug!("Bithumb: Sent UDP packet for {} trade at price {}", trade.symbol, trade.price);
                    }
                }

                crate::core::COMPARE_NOTIFY.notify_waiters();
            },
            "orderbook" => {
                // Orderbook format from Bithumb - symbol is already in KRW-BTC format
                let original_symbol = value.get("code")
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                // No conversion needed - use symbol directly
                let common_symbol = match symbol_mapper.map("Bithumb", original_symbol) {
                    Some(symbol) => symbol,
                    None => {
                        if count % 100 == 0 {
                            debug!("[Bithumb] No mapping found for orderbook symbol: {}",
                                original_symbol);
                        }
                        return; // Skip this message if no mapping found
                    }
                };

                // HFT: Get precision BEFORE parsing orderbook
                let price_precision = config.feed_config.get_price_precision(&common_symbol);
                let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                // Parse orderbook data
                let orderbook_units = value.get("orderbook_units")
                    .and_then(Value::as_array);

                if let Some(units) = orderbook_units {
                    let mut bids = Vec::new();
                    let mut asks = Vec::new();

                    for unit in units.iter().take(20) {
                        if let Some(ask_price) = unit.get("ask_price").and_then(Value::as_f64) {
                            if let Some(ask_size) = unit.get("ask_size").and_then(Value::as_f64) {
                                let price = (ask_price * 10_f64.powi(price_precision as i32)) as i64;
                                let size = (ask_size * 10_f64.powi(qty_precision as i32)) as i64;
                                asks.push((price, size));
                            }
                        }
                        if let Some(bid_price) = unit.get("bid_price").and_then(Value::as_f64) {
                            if let Some(bid_size) = unit.get("bid_size").and_then(Value::as_f64) {
                                let price = (bid_price * 10_f64.powi(price_precision as i32)) as i64;
                                let size = (bid_size * 10_f64.powi(qty_precision as i32)) as i64;
                                bids.push((price, size));
                            }
                        }
                    }

                    let timestamp = value.get("timestamp")
                        .and_then(Value::as_i64)
                        .map(|t| t.max(0) as u64)
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);

                    if count % 50 == 0 {
                        debug!("Bithumb ORDERBOOK {} - {} bids, {} asks", common_symbol, bids.len(), asks.len());
                    }

                    let orderbook = crate::core::OrderBookData {
                        exchange: "Bithumb".to_string(),
                        symbol: common_symbol,
                        asset_type: asset_type.to_string(),
                        bids,
                        asks,
                        price_precision,
                        quantity_precision: qty_precision,
                        timestamp,
                        timestamp_unit: config.feed_config.timestamp_unit,
                    };

                    {
                        let mut orderbooks_lock = orderbooks.write();
                        orderbooks_lock.insert(orderbook.symbol.clone(), orderbook.clone());
                    }

                    // Send UDP packet for orderbook
                    if let Some(sender) = crate::core::get_multi_port_sender() {
                        let _ = sender.send_orderbook_data(orderbook);
                    }
                }
            },
            _ => {
                if count % 100 == 0 {
                    debug!("[Bithumb] Unhandled message type: {}", msg_type);
                }
            }
        }
    } else {
        // Old format fallback (shouldn't happen with new API)
        if let Some(content) = value.get("content") {
            if let Some(list) = content.get("list").and_then(Value::as_array) {
                debug!("Bithumb: Received legacy format with {} items", list.len());
            }
        }
    }
}

