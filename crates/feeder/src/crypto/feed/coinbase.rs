
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

use crate::core::{OrderBookData, Feeder, SymbolMapper, CONNECTION_STATS, get_shutdown_receiver, parse_to_scaled_or_default, MultiPortUdpSender, get_multi_port_sender};
use crate::error::Result;
use crate::load_config::ExchangeConfig;

pub struct CoinbaseExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    ws_streams: HashMap<(String, String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    active_connections: Arc<AtomicUsize>,
    orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>,
    multi_port_sender: Option<Arc<MultiPortUdpSender>>,
}

impl CoinbaseExchange {
    fn ensure_sender(&mut self) {
        if self.multi_port_sender.is_none() {
            if let Some(sender) = get_multi_port_sender() {
                self.multi_port_sender = Some(sender);
                info!("CoinbaseExchange: multi_port_sender initialized");
            }
        }
    }

    pub fn new(config: ExchangeConfig, symbol_mapper: Arc<SymbolMapper>) -> Self {
        const SYMBOLS_PER_CONNECTION: usize = 10;
        let mut ws_streams = HashMap::new();

        let num_symbols = config.subscribe_data.spot_symbols.len();
        let num_chunks = (num_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;

        debug!("Coinbase constructor - {} symbols, {} per connection = {} connections per stream type",
            num_symbols, SYMBOLS_PER_CONNECTION, num_chunks);

        // Initialize separate connections for trade and orderbook streams
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
            multi_port_sender: None, // Will be set in ensure_sender()
        }
    }
}

#[async_trait]
impl Feeder for CoinbaseExchange {
    fn name(&self) -> &str {
        "Coinbase"
    }

    async fn connect(&mut self) -> Result<()> {
        let config = self.config.clone();

        info!("Coinbase: Creating {} WebSocket connections (10 symbols each, separate for trade and orderbook)", self.ws_streams.len());

        for ((asset_type, stream_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            let ws_url = "wss://advanced-trade-ws.coinbase.com";
            debug!("Connecting to Coinbase {} {} chunk {}: {}", asset_type, stream_type, chunk_idx, ws_url);
            
            let mut retry_count = 0;
            let max_retries = 5;
            let mut retry_delay = Duration::from_secs(config.connect_config.initial_retry_delay_secs);
            
            loop {
                match connect_with_large_buffer(ws_url).await {
                    Ok((stream, _)) => {
                        *ws_stream = Some(stream);
                        info!("Successfully connected to Coinbase {} {} chunk {} at {}",
                            asset_type, stream_type, chunk_idx, ws_url);

                        // Update connection stats - mark as connected
                        {
                            let mut stats = CONNECTION_STATS.write();
                            let key = format!("Coinbase_{}_{}", asset_type, stream_type);
                            let entry = stats.entry(key.clone()).or_default();
                            entry.connected += 1;
                            entry.total_connections += 1;
                            info!("[{}] Connection established for chunk {}", key, chunk_idx);
                        }

                        debug!("Connected to Coinbase {} {} chunk {}", asset_type, stream_type, chunk_idx);
                        break;
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        retry_count += 1;

                        warn!("Connection error for Coinbase {} {} chunk {}: {}. Retrying in {} seconds (attempt {}/{})",
                            asset_type, stream_type, chunk_idx, error_str, retry_delay.as_secs(), retry_count, max_retries);

                        if retry_count > max_retries {
                            warn!("Failed to connect to Coinbase {} {} chunk {} after {} retries. Skipping this connection.",
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

                // Subscribe to ONLY the channel for this stream type
                let channel = if stream_type == "trade" {
                    "market_trades"
                } else {
                    "level2"
                };

                let subscribe_msg = json!({
                    "type": "subscribe",
                    "product_ids": chunk_symbols,
                    "channel": channel
                });

                let subscription = subscribe_msg.to_string();
                info!("Sending {} subscription to Coinbase {} chunk {} with {} symbols",
                    channel, asset_type, chunk_idx, chunk_symbols.len());
                debug!("Subscription details: {}", subscription);

                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;

                info!("Sent {} subscription to Coinbase {} chunk {} with symbols: {:?}",
                    stream_type, asset_type, chunk_idx, chunk_symbols);
            }
        }
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        // Ensure we have a sender before starting
        self.ensure_sender();

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
                let multi_port_sender = self.multi_port_sender.clone();

                active_connections.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    debug!("Starting Coinbase {} {} chunk {} stream processing",
                        asset_type, stream_type, chunk_idx);
                    
                    let mut shutdown_rx = get_shutdown_receiver();
                    
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
                    
                    let mut last_message_time = std::time::Instant::now();
                    let mut health_check_interval = tokio::time::interval(Duration::from_secs(60));
                    
                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Coinbase {} {} chunk {} received shutdown signal", asset_type, stream_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Coinbase {} {} chunk {}: {}",
                                        asset_type, stream_type, chunk_idx, e);
                                    error!("[Coinbase_{}_{}] WebSocket ping failed for chunk {}", asset_type, stream_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to Coinbase {} {} chunk {}",
                                    asset_type, stream_type, chunk_idx);
                            }
                            _ = health_check_interval.tick() => {
                                if last_message_time.elapsed() > Duration::from_secs(300) { // 5 minutes
                                    warn!("No messages received from Coinbase {} {} chunk {} for {} seconds, connection may be stale",
                                        asset_type, stream_type, chunk_idx, last_message_time.elapsed().as_secs());

                                    if let Err(e) = ws_stream.send(Message::Ping(b"health_check".to_vec().into())).await {
                                        error!("Health check ping failed for Coinbase {} {} chunk {}: {}",
                                            asset_type, stream_type, chunk_idx, e);
                                        break;
                                    }
                                }
                            }
                            msg = ws_stream.next().fuse() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        last_message_time = std::time::Instant::now();
                                        process_coinbase_message(&text, symbol_mapper.clone(), &asset_type, orderbooks.clone(), &config, multi_port_sender.clone());
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        last_message_time = std::time::Instant::now();
                                        debug!("Received pong from Coinbase {} {} chunk {}",
                                            asset_type, stream_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Ping(data))) => {
                                        last_message_time = std::time::Instant::now();
                                        if let Err(e) = ws_stream.send(Message::Pong(data)).await {
                                            warn!("Failed to send pong to Coinbase {} {} chunk {}: {}",
                                                asset_type, stream_type, chunk_idx, e);
                                            break;
                                        }
                                        debug!("Sent pong response to Coinbase {} {} chunk {}",
                                            asset_type, stream_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("Coinbase WebSocket closed for {} {} chunk {}: {:?}",
                                            asset_type, stream_type, chunk_idx, frame);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Coinbase_{}_{}", asset_type, stream_type);
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
                                        debug!("Received other message type from Coinbase: {:?}", msg);
                                    },
                                    Some(Err(e)) => {
                                        warn!("Coinbase WebSocket error for {} {} chunk {}: {}",
                                            asset_type, stream_type, chunk_idx, e);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Coinbase_{}_{}", asset_type, stream_type);
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
                                        warn!("WebSocket stream ended for Coinbase {} {} chunk {}",
                                            asset_type, stream_type, chunk_idx);
                                        {
                                            let mut stats = CONNECTION_STATS.write();
                                            let key = format!("Coinbase_{}_{}", asset_type, stream_type);
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
                    warn!("WebSocket connection lost for Coinbase {} {} chunk {}, will be retried by feeder",
                        asset_type, stream_type, chunk_idx);

                    active_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
        }

        while self.active_connections.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        warn!("All Coinbase connections lost, returning to trigger reconnection");
        Ok(())
    }
}

fn process_coinbase_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>, config: &ExchangeConfig, multi_port_sender: Option<Arc<MultiPortUdpSender>>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static MESSAGE_COUNT: AtomicU64 = AtomicU64::new(0);

    let count = MESSAGE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Only log initial messages for debugging connection, then periodic summaries
    if count < 5 {
        debug!("Coinbase initial message #{}: {}", count, &text[..text.len().min(200)]);
    } else if count % 10000 == 0 {
        debug!("Coinbase: Processed {} messages so far", count);
    }

    // Only log truly unusual messages (not market data)
    if !text.contains("ticker") && !text.contains("update") && !text.contains("snapshot") && !text.contains("market_trades")
        && !text.contains("subscriptions") && !text.contains("heartbeat") && !text.contains("l2_data") {
        debug!("Coinbase non-standard message type: {}",
            text.get(0..100).unwrap_or(text));
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", crate::core::safe_json_parse_error_log(&e, text));
            return;
        }
    };

    // Check for market_trades channel messages (Advanced Trade API format)
    if let Some(channel) = value.get("channel").and_then(Value::as_str) {
        if channel == "market_trades" {
            if let Some(events) = value.get("events").and_then(Value::as_array) {
                for event in events {
                    if let Some(trades_arr) = event.get("trades").and_then(Value::as_array) {
                        for trade_data in trades_arr {
                            let original_symbol = trade_data.get("product_id").and_then(Value::as_str).unwrap_or_default();
                            let common_symbol = match symbol_mapper.map("Coinbase", original_symbol) {
                                Some(symbol) => symbol,
                                None => original_symbol.to_string()
                            };

                            // HFT: Get precision from config
                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            // HFT: Parse directly to scaled i64 (NO f64!)
                            let price = trade_data.get("price")
                                .and_then(Value::as_str)
                                .map(|s| parse_to_scaled_or_default(s, price_precision))
                                .unwrap_or(0);
                            let quantity = trade_data.get("size")
                                .and_then(Value::as_str)
                                .map(|s| parse_to_scaled_or_default(s, qty_precision))
                                .unwrap_or(0);
                            let timestamp = trade_data.get("time")
                                .and_then(Value::as_str)
                                .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
                                .map(|dt| dt.timestamp_millis().max(0) as u64)
                                .unwrap_or_default();

                            let trade = crate::core::TradeData {
                                exchange: "Coinbase".to_string(),
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

                            if let Some(sender) = &multi_port_sender {
                                match sender.send_trade_data(trade.clone()) {
                                    Ok(_) => {
                                        // Log success only occasionally to avoid log spam
                                        if count % 5000 == 0 {
                                            debug!("Coinbase: Successfully sending trades via UDP (sample: {} @ {})", trade.symbol, trade.price);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Coinbase: Failed to send UDP packet for {} trade: {}", trade.symbol, e);
                                    }
                                }
                            } else {
                                // Log error only occasionally to avoid log spam when sender is not ready
                                if count % 500 == 0 {
                                    warn!("Coinbase: Multi-port sender not available for trade data");
                                }
                            }

                            crate::core::COMPARE_NOTIFY.notify_waiters();
                        }
                    }
                }
            }
            return; // Early return after handling market_trades
        }
    }

    // Check for l2_data channel messages (Advanced Trade API format for level2)
    if let Some(channel) = value.get("channel").and_then(Value::as_str) {
        if channel == "l2_data" {
            if let Some(events) = value.get("events").and_then(Value::as_array) {
                for event in events {
                    let msg_type = event.get("type").and_then(Value::as_str).unwrap_or("");
                    let product_id = event.get("product_id").and_then(Value::as_str).unwrap_or("");

                    // Map to common symbol
                    let common_symbol = match symbol_mapper.map("Coinbase", product_id) {
                        Some(symbol) => symbol,
                        None => product_id.to_string()
                    };

                    match msg_type {
                        "snapshot" => {
                            // Process snapshot
                            if count % 100 == 0 || count < 10 {
                                info!("Coinbase: Received l2_data snapshot for {}", product_id);
                            }

                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            let mut bids: Vec<(i64, i64)> = Vec::new();
                            let mut asks: Vec<(i64, i64)> = Vec::new();

                            if let Some(updates) = event.get("updates").and_then(Value::as_array) {
                                for update in updates {
                                    let side = update.get("side").and_then(Value::as_str).unwrap_or("");
                                    let price_str = update.get("price_level").and_then(Value::as_str).unwrap_or("0");
                                    let qty_str = update.get("new_quantity").and_then(Value::as_str).unwrap_or("0");

                                    let price = parse_to_scaled_or_default(price_str, price_precision);
                                    let qty = parse_to_scaled_or_default(qty_str, qty_precision);

                                    if side == "bid" {
                                        bids.push((price, qty));
                                    } else if side == "ask" {
                                        asks.push((price, qty));
                                    }
                                }
                            }

                            bids.sort_by(|a, b| b.0.cmp(&a.0)); // Sort bids descending
                            asks.sort_by(|a, b| a.0.cmp(&b.0)); // Sort asks ascending

                            let orderbook = crate::core::OrderBookData {
                                exchange: "Coinbase".to_string(),
                                symbol: common_symbol.clone(),
                                asset_type: asset_type.to_string(),
                                bids,
                                asks,
                                price_precision,
                                quantity_precision: qty_precision,
                                timestamp: 0,
                                timestamp_unit: config.feed_config.timestamp_unit,
                            };

                            {
                                let mut orderbooks = orderbooks.write();
                                orderbooks.insert(common_symbol.clone(), orderbook.clone());
                            }

                            if let Some(sender) = &multi_port_sender {
                                info!("Coinbase: Sending l2_data snapshot for {} - {} bids, {} asks",
                                    orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());

                                if let Err(e) = sender.send_orderbook_data(orderbook.clone()) {
                                    error!("Coinbase: Failed to send l2_data snapshot UDP: {}", e);
                                }
                            }
                        }
                        "update" => {
                            // Process update
                            if count % 1000 == 0 || count < 10 {
                                info!("Coinbase: Received l2_data update for {}", product_id);
                            }

                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            let mut orderbooks = orderbooks.write();
                            let orderbook = orderbooks.entry(common_symbol.clone()).or_insert_with(|| {
                                info!("Coinbase: Creating new orderbook for {} from l2_data update", common_symbol);
                                crate::core::OrderBookData {
                                    exchange: "Coinbase".to_string(),
                                    symbol: common_symbol.clone(),
                                    asset_type: asset_type.to_string(),
                                    bids: Vec::new(),
                                    asks: Vec::new(),
                                    price_precision,
                                    quantity_precision: qty_precision,
                                    timestamp: 0,
                                    timestamp_unit: config.feed_config.timestamp_unit,
                                }
                            });

                            if let Some(updates) = event.get("updates").and_then(Value::as_array) {
                                for update in updates {
                                    let side = update.get("side").and_then(Value::as_str).unwrap_or("");
                                    let price_str = update.get("price_level").and_then(Value::as_str).unwrap_or("0");
                                    let qty_str = update.get("new_quantity").and_then(Value::as_str).unwrap_or("0");

                                    let price = parse_to_scaled_or_default(price_str, price_precision);
                                    let qty = parse_to_scaled_or_default(qty_str, qty_precision);

                                    if side == "bid" {
                                        let book_side = &mut orderbook.bids;
                                        if qty == 0 {
                                            book_side.retain(|(p, _)| *p != price);
                                        } else {
                                            if let Some(entry) = book_side.iter_mut().find(|(p, _)| *p == price) {
                                                entry.1 = qty;
                                            } else {
                                                book_side.push((price, qty));
                                            }
                                        }
                                        book_side.sort_by(|a, b| b.0.cmp(&a.0));
                                    } else if side == "ask" {
                                        let book_side = &mut orderbook.asks;
                                        if qty == 0 {
                                            book_side.retain(|(p, _)| *p != price);
                                        } else {
                                            if let Some(entry) = book_side.iter_mut().find(|(p, _)| *p == price) {
                                                entry.1 = qty;
                                            } else {
                                                book_side.push((price, qty));
                                            }
                                        }
                                        book_side.sort_by(|a, b| a.0.cmp(&b.0));
                                    }
                                }
                            }

                            if let Some(sender) = &multi_port_sender {
                                if count < 10 || count % 1000 == 0 {
                                    info!("Coinbase: Sending l2_data update for {} - {} bids, {} asks",
                                        orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());
                                }

                                if let Err(e) = sender.send_orderbook_data(orderbook.clone()) {
                                    error!("Coinbase: Failed to send l2_data update UDP: {}", e);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            return; // Early return after handling l2_data
        }
    }

    if let Some(msg_type) = value.get("type").and_then(Value::as_str) {
        if count % 50 == 0 || msg_type != "ticker" && msg_type != "l2update" && msg_type != "update" {
            // println!("Coinbase message type: {} (total: {})", msg_type, count);
        }
        match msg_type {
            "ticker" => {
                let original_symbol = value.get("product_id").and_then(Value::as_str).unwrap_or_default();
                let common_symbol = match symbol_mapper.map("Coinbase", original_symbol) {
                    Some(symbol) => symbol,
                    None => original_symbol.to_string()
                };

                // HFT: Get precision from config
                let price_precision = config.feed_config.get_price_precision(&common_symbol);
                let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                // HFT: Parse directly to scaled i64 (NO f64!)
                let price = value.get("price")
                    .and_then(Value::as_str)
                    .map(|s| parse_to_scaled_or_default(s, price_precision))
                    .unwrap_or(0);
                let quantity = value.get("last_size")
                    .and_then(Value::as_str)
                    .map(|s| parse_to_scaled_or_default(s, qty_precision))
                    .unwrap_or(0);
                let timestamp = value.get("time")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
                    .map(|dt| dt.timestamp_millis().max(0) as u64)
                    .unwrap_or_default();

                // println!("TRADE {} @ {} x {}", common_symbol, price, quantity);

                let trade = crate::core::TradeData {
                    exchange: "Coinbase".to_string(),
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
                    let mut trades = crate::core::TRADES.write();
                    trades.push(trade.clone());
                }

                if let Some(sender) = &multi_port_sender {
                    if let Err(e) = sender.send_trade_data(trade.clone()) {
                        error!("Coinbase: Failed to send ticker trade UDP: {}", e);
                    } else if count % 5000 == 0 {
                        debug!("Coinbase: Successfully sending ticker trades via UDP (sample: {} @ {})", trade.symbol, trade.price);
                    }
                } else if count % 500 == 0 {
                    warn!("Coinbase: Multi-port sender not available for ticker trade data");
                }

                crate::core::COMPARE_NOTIFY.notify_waiters();
            }
            "snapshot" => {
                let original_symbol = value.get("product_id").and_then(Value::as_str).unwrap_or_default();

                // Log snapshot reception
                if count % 100 == 0 || count < 10 {
                    info!("Coinbase: Received snapshot for {}", original_symbol);
                }

                let common_symbol = match symbol_mapper.map("Coinbase", original_symbol) {
                    Some(symbol) => symbol,
                    None => {
                        // Use original symbol if no mapping exists
                        original_symbol.to_string()
                    }
                };

                // HFT: Get precision BEFORE parsing orderbook
                let price_precision = config.feed_config.get_price_precision(&common_symbol);
                let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                // HFT: Parse directly to scaled i64 (NO f64!)
                let bids: Vec<(i64, i64)> = value.get("bids")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter().filter_map(|level| {
                            let price_str = level.get(0)?.as_str()?;
                            let qty_str = level.get(1)?.as_str()?;
                            let price = parse_to_scaled_or_default(price_str, price_precision);
                            let qty = parse_to_scaled_or_default(qty_str, qty_precision);
                            Some((price, qty))
                        }).collect()
                    })
                    .unwrap_or_default();

                let asks: Vec<(i64, i64)> = value.get("asks")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter().filter_map(|level| {
                            let price_str = level.get(0)?.as_str()?;
                            let qty_str = level.get(1)?.as_str()?;
                            let price = parse_to_scaled_or_default(price_str, price_precision);
                            let qty = parse_to_scaled_or_default(qty_str, qty_precision);
                            Some((price, qty))
                        }).collect()
                    })
                    .unwrap_or_default();

                let orderbook = crate::core::OrderBookData {
                    exchange: "Coinbase".to_string(),
                    symbol: common_symbol.clone(),
                    asset_type: asset_type.to_string(),
                    bids: bids.clone(),
                    asks: asks.clone(),
                    price_precision,
                    quantity_precision: qty_precision,
                    timestamp: 0, // Coinbase snapshot does not provide a timestamp
                    timestamp_unit: config.feed_config.timestamp_unit,
                };

                // println!("ORDERBOOK {} ({}L)", common_symbol, bids.len() + asks.len());

                {
                    let mut orderbooks = orderbooks.write();
                    orderbooks.insert(common_symbol, orderbook.clone());
                }

                if let Some(sender) = &multi_port_sender {
                    // Always log for debugging
                    info!("Coinbase: Sending orderbook snapshot for {} - {} bids, {} asks",
                        orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());

                    if let Err(e) = sender.send_orderbook_data(orderbook.clone()) {
                        error!("Coinbase: Failed to send orderbook snapshot UDP: {}", e);
                    } else if count % 5000 == 0 {
                        debug!("Coinbase: Successfully sending orderbook snapshots via UDP (sample: {} with {} bids, {} asks)",
                            orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());
                    }
                } else if count % 500 == 0 {
                    warn!("Coinbase: Multi-port sender not available for orderbook snapshot data");
                }
                
                crate::core::COMPARE_NOTIFY.notify_waiters();
            }
            "l2update" => {
                // Note: Advanced Trade API doesn't send l2update, it sends "update" in l2_data channel
                // This is kept for compatibility with Exchange API (legacy Coinbase Pro)
                let original_symbol = value.get("product_id").and_then(Value::as_str).unwrap_or_default();

                // Log l2update reception periodically
                if count % 1000 == 0 || count < 10 {
                    info!("Coinbase: Received l2update for {}", original_symbol);
                }

                // Extra debug for first few messages
                if count < 20 {
                    info!("Coinbase l2update #{} for {}, sender available: {}",
                        count, original_symbol, multi_port_sender.is_some());
                }

                let common_symbol = match symbol_mapper.map("Coinbase", original_symbol) {
                    Some(symbol) => symbol,
                    None => {
                        // Use original symbol if no mapping exists
                        original_symbol.to_string()
                    }
                };

                // HFT: Get precision for updates
                let price_precision = config.feed_config.get_price_precision(&common_symbol);
                let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                let mut orderbooks = orderbooks.write();

                // Get or create orderbook if it doesn't exist yet
                let orderbook = orderbooks.entry(common_symbol.clone()).or_insert_with(|| {
                    info!("Coinbase: Creating new orderbook for {} from l2update", common_symbol);
                    crate::core::OrderBookData {
                        exchange: "Coinbase".to_string(),
                        symbol: common_symbol.clone(),
                        asset_type: asset_type.to_string(),
                        bids: Vec::new(),
                        asks: Vec::new(),
                        price_precision,
                        quantity_precision: qty_precision,
                        timestamp: 0,
                        timestamp_unit: config.feed_config.timestamp_unit,
                    }
                });

                // Now update the orderbook
                    let timestamp = value.get("time")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
                        .map(|dt| dt.timestamp_millis().max(0) as u64)
                        .unwrap_or_default();
                    orderbook.timestamp = timestamp;

                    if let Some(changes) = value.get("changes").and_then(Value::as_array) {
                        for change in changes {
                            if let Some(change_arr) = change.as_array() {
                                if change_arr.len() == 3 {
                                    let side = change_arr[0].as_str().unwrap_or_default();
                                    let price = change_arr[1].as_str()
                                        .map(|s| parse_to_scaled_or_default(s, price_precision))
                                        .unwrap_or(0);
                                    let quantity = change_arr[2].as_str()
                                        .map(|s| parse_to_scaled_or_default(s, qty_precision))
                                        .unwrap_or(0);

                                    if side == "buy" {
                                        let book_side = &mut orderbook.bids;
                                        if quantity == 0 {
                                            book_side.retain(|(p, _)| *p != price);
                                        } else {
                                            if let Some(entry) = book_side.iter_mut().find(|(p, _)| *p == price) {
                                                entry.1 = quantity;
                                            } else {
                                                book_side.push((price, quantity));
                                            }
                                        }
                                        book_side.sort_by(|a, b| b.0.cmp(&a.0));
                                    } else {
                                        let book_side = &mut orderbook.asks;
                                        if quantity == 0 {
                                            book_side.retain(|(p, _)| *p != price);
                                        } else {
                                            if let Some(entry) = book_side.iter_mut().find(|(p, _)| *p == price) {
                                                entry.1 = quantity;
                                            } else {
                                                book_side.push((price, quantity));
                                            }
                                        }
                                        book_side.sort_by(|a, b| a.0.cmp(&b.0));
                                    }
                                }
                            }
                        }
                    }

                    if let Some(sender) = &multi_port_sender {
                        // Log first few updates and periodic samples
                        if count < 10 || count % 1000 == 0 {
                            info!("Coinbase: Sending l2update for {} - {} bids, {} asks",
                                orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());
                        }

                        if let Err(e) = sender.send_orderbook_data(orderbook.clone()) {
                            error!("Coinbase: Failed to send orderbook update UDP: {}", e);
                        } else if count % 5000 == 0 {
                            debug!("Coinbase: Successfully sending orderbook updates via UDP (sample: {} with {} bids, {} asks)",
                                orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());
                        }
                    } else if count % 500 == 0 {
                        warn!("Coinbase: Multi-port sender not available for orderbook update data");
                    }

                crate::core::COMPARE_NOTIFY.notify_waiters();
            }
            "error" => {
                let error_msg = value.get("message").and_then(Value::as_str).unwrap_or("Unknown error");
                error!("Coinbase error: {}", error_msg);
                error!("Coinbase error response: {}", text);
            }
            _ => {
                debug!("[Coinbase] Unhandled message type: {}", msg_type);
            }
        }
    }
}
