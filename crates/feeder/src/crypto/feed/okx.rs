use async_trait::async_trait;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tracing::{debug, info, warn, error};
use super::websocket_config::connect_with_large_buffer;
use chrono;

use crate::core::{Feeder, TRADES, ORDERBOOKS, COMPARE_NOTIFY, TradeData, OrderBookData, SymbolMapper, get_shutdown_receiver, CONNECTION_STATS, get_multi_port_sender, parse_to_scaled_or_default};
use crate::core::robust_connection::ExchangeConnectionLimits;
use crate::error::{Result, Error};
use crate::load_config::ExchangeConfig;
use crate::connect_to_databse::ConnectionEvent;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const USE_MULTI_PORT_UDP: bool = true;

pub struct OkxExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    // Map of (asset_type, chunk_index) -> WebSocket connection
    // Each connection handles up to 5 symbols with both trade and orderbook streams
    ws_streams: HashMap<(String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    // Store subscription messages for reconnection
    subscription_messages: HashMap<(String, usize), String>,
    // Track active connections for proper reconnection
    active_connections: Arc<AtomicUsize>,
}

impl OkxExchange {
    pub fn new(
        config: ExchangeConfig,
        symbol_mapper: Arc<SymbolMapper>,
    ) -> Self {
        let limits = ExchangeConnectionLimits::for_exchange("okx");
        let symbols_per_connection = limits.max_symbols_per_connection;
        let mut ws_streams = HashMap::new();

        // Calculate how many connections we need for each asset type
        // Initialize connections for each asset type separately
        for asset_type in &config.feed_config.asset_type {
            let symbols = if asset_type == "spot" {
                &config.subscribe_data.spot_symbols
            } else {
                &config.subscribe_data.futures_symbols
            };

            let num_symbols = symbols.len();
            let num_chunks = (num_symbols + symbols_per_connection - 1) / symbols_per_connection;

            // Create connection entries for each chunk
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
            subscription_messages: HashMap::new(),
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }

    async fn connect_websocket(&mut self, asset_type: &str, chunk_idx: usize) -> Result<()> {
        let limits = ExchangeConnectionLimits::for_exchange("okx");
        let symbols_per_connection = limits.max_symbols_per_connection;

        // Get symbols based on asset type
        let symbols = if asset_type == "spot" {
            &self.config.subscribe_data.spot_symbols
        } else {
            &self.config.subscribe_data.futures_symbols
        };

        // Get the symbols for this chunk (5 symbols per connection)
        let start_idx = chunk_idx * symbols_per_connection;
        let end_idx = std::cmp::min(start_idx + symbols_per_connection, symbols.len());
        let chunk_symbols = &symbols[start_idx..end_idx];

        // OKX uses a single WebSocket URL for all connections
        let ws_url = "wss://ws.okx.com:8443/ws/v5/public";

        info!("OKX: Connecting to {} chunk {} with {} symbols", asset_type, chunk_idx, chunk_symbols.len());
        debug!("  Symbols: {:?}", chunk_symbols);

        match connect_with_large_buffer(ws_url).await {
            Ok((ws_stream, response)) => {
                debug!("OKX WebSocket connected for {} chunk {}", asset_type, chunk_idx);
                debug!("OKX Connection response: {:?}", response);

                // Update connection stats - mark as connected
                {
                    let mut stats = CONNECTION_STATS.write();
                    let key = format!("OKX_{}", asset_type);
                    let entry = stats.entry(key.clone()).or_default();
                    entry.connected += 1;
                    entry.total_connections += 1;
                    info!("OKX: [{}] Connection established for chunk {}", key, chunk_idx);
                }

                // Log successful connection to QuestDB
                let connection_id = format!("okx-{}-{}", asset_type, chunk_idx);
                let _ = crate::core::feeder_metrics::log_connection(
                    "OKX",
                    &connection_id,
                    ConnectionEvent::Connected
                ).await;

                if let Some(stream_slot) = self.ws_streams.get_mut(&(asset_type.to_string(), chunk_idx)) {
                    *stream_slot = Some(ws_stream);
                }
                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to OKX WebSocket for {} chunk {}: {}",
                    asset_type, chunk_idx, e);
                Err(Error::Connection(format!("OKX connection failed: {}", e)))
            }
        }
    }
}

#[async_trait]
impl Feeder for OkxExchange {
    fn name(&self) -> &str {
        "OKX"
    }

    async fn connect(&mut self) -> Result<()> {
        let mut retry_delay = Duration::from_secs(self.config.connect_config.initial_retry_delay_secs);
        let connection_keys: Vec<(String, usize)> = self.ws_streams
            .keys()
            .map(|(asset, chunk)| (asset.clone(), *chunk))
            .collect();

        info!("OKX: Creating {} WebSocket connections (5 symbols each, with both trade and orderbook streams)", connection_keys.len());

        for (asset_type, chunk_idx) in connection_keys {
            loop {
                match self.connect_websocket(&asset_type, chunk_idx).await {
                    Ok(()) => {
                        debug!("Successfully connected to OKX {} chunk {}",
                            asset_type, chunk_idx);
                        retry_delay = Duration::from_secs(self.config.connect_config.initial_retry_delay_secs);  // Reset delay on success
                        break;  // Move to next connection
                    },
                    Err(e) => {
                        error!("Failed to connect to OKX {} chunk {}: {}",
                            asset_type, chunk_idx, e);
                        debug!("Retrying in {} seconds...", retry_delay.as_secs());
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = retry_delay.saturating_mul(2).min(Duration::from_secs(self.config.connect_config.max_retry_delay_secs));
                        // Continue trying this connection
                    }
                }
            }
            // Configurable delay between establishing connections to avoid overwhelming the server
            tokio::time::sleep(Duration::from_millis(self.config.connect_config.connection_delay_ms)).await;
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<()> {
        let limits = ExchangeConnectionLimits::for_exchange("okx");
        let symbols_per_connection = limits.max_symbols_per_connection;

        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            if let Some(ws_stream) = ws_stream {
                // Get the symbols for this chunk
                // Get symbols based on asset type
                let symbols = if asset_type == "spot" {
                    &self.config.subscribe_data.spot_symbols
                } else {
                    &self.config.subscribe_data.futures_symbols
                };

                let start_idx = *chunk_idx * symbols_per_connection;
                let end_idx = std::cmp::min(start_idx + symbols_per_connection, symbols.len());
                let chunk_symbols = &symbols[start_idx..end_idx];

                // Build subscription arguments for OKX
                let mut args = Vec::new();
                for symbol in chunk_symbols {
                    // Add trade channel subscription
                    args.push(json!({
                        "channel": "trades",
                        "instId": symbol
                    }));

                    // Add orderbook channel subscription
                    // OKX supports: books (400 levels), books5 (5 levels), books-l2-tbt (full depth tick-by-tick), books50-l2-tbt (50 levels)
                    args.push(json!({
                        "channel": "books5",
                        "instId": symbol
                    }));
                }

                // Create subscription message in OKX format
                // OKX doesn't require an ID field for subscriptions
                let subscribe_msg = json!({
                    "op": "subscribe",
                    "args": args
                });

                let subscription = subscribe_msg.to_string();
                debug!("Sending subscription to OKX {} chunk {} with {} symbols",
                    asset_type, chunk_idx, chunk_symbols.len());
                debug!("  Subscription message: {}", subscription);

                // Store subscription for potential reconnection
                self.subscription_messages.insert(
                    (asset_type.clone(), *chunk_idx),
                    subscription.clone()
                );

                let message = Message::Text(subscription.into());
                ws_stream.send(message).await?;
                info!("OKX: Sent subscription to {} chunk {} with {} symbols",
                    asset_type, chunk_idx, chunk_symbols.len());
                debug!("  Subscribed symbols: {:?}", chunk_symbols);
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

                // Increment active connection count BEFORE spawning
                active_connections.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    info!("OKX: Starting {} chunk {} stream processing (trade + orderbook)",
                        asset_type, chunk_idx);

                    // Get shutdown receiver
                    let mut shutdown_rx = get_shutdown_receiver();

                    // Add periodic ping to keep connection alive
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));

                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("OKX {} chunk {} received shutdown signal", asset_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                // Send ping to keep connection alive (OKX format)
                                let ping_msg = "ping";
                                if let Err(e) = ws_stream.send(Message::Text(ping_msg.into())).await {
                                    warn!("Failed to send ping to OKX {} chunk {}: {}",
                                        asset_type, chunk_idx, e);
                                    error!("Connection lost for OKX {} chunk {}",
                                        asset_type, chunk_idx);

                                    // Log disconnection to QuestDB
                                    let connection_id = format!("okx-{}-{}", asset_type, chunk_idx);
                                    let _ = crate::core::feeder_metrics::log_connection(
                                        "OKX",
                                        &connection_id,
                                        ConnectionEvent::Disconnected {
                                            reason: "ping failed".to_string()
                                        }
                                    ).await;
                                    error!("[OKX_{}] WebSocket disconnected for chunk {} - ping failed", asset_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to OKX {} chunk {}",
                                    asset_type, chunk_idx);
                            }
                            msg = ws_stream.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        // Add debug log to see all messages
                                        if !text.contains("pong") && !text.contains("heartbeat") {
                                            debug!("OKX {} chunk {} received: {}", asset_type, chunk_idx,
                                                if text.len() > 200 { &text[..200] } else { &text });
                                        }
                                        process_okx_message(&text, symbol_mapper.clone(), &asset_type, &config);
                                    },
                                    Some(Ok(Message::Ping(ping))) => {
                                        if let Err(e) = ws_stream.send(Message::Pong(ping)).await {
                                            debug!("Failed to send pong: {}", e);
                                            break;
                                        }
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        debug!("Received pong from OKX {} chunk {}",
                                            asset_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("WebSocket closed by OKX server: {:?}", frame);
                                        error!("[OKX_{}] WebSocket issue for chunk {}", asset_type, chunk_idx);
                                        break;
                                    },
                                    Some(Ok(_)) => {
                                        // Handle other message types if needed
                                    },
                                    Some(Err(e)) => {
                                        let error_str = e.to_string();
                                        if error_str.contains("10054") || error_str.contains("forcibly closed") {
                                            warn!("OKX {} chunk {} connection forcibly closed by remote host", asset_type, chunk_idx);
                                        } else {
                                            error!("WebSocket error for OKX {} chunk {}: {}",
                                                asset_type, chunk_idx, e);
                                        }
                                        error!("[OKX_{}] WebSocket issue for chunk {}", asset_type, chunk_idx);
                                        break;
                                    },
                                    None => {
                                        warn!("WebSocket stream ended for OKX {} chunk {}",
                                            asset_type, chunk_idx);
                                        error!("[OKX_{}] WebSocket issue for chunk {}", asset_type, chunk_idx);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    warn!("OKX {} chunk {} disconnected. Connection will be retried by feeder.",
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

        warn!("All OKX connections lost, returning to trigger reconnection");
        Ok(())
    }
}

fn process_okx_message(text: &str, symbol_mapper: Arc<SymbolMapper>, asset_type: &str, config: &ExchangeConfig) {
    // Handle OKX's "pong" response
    if text == "pong" {
        // Silent - don't log pongs
        return;
    }

    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", crate::core::safe_json_parse_error_log(&e, text));
            return;
        }
    };

    // Handle OKX subscription responses and errors
    if let Some(event) = value.get("event").and_then(|v| v.as_str()) {
        match event {
            "subscribe" => {
                info!("OKX: Subscription confirmed");
                debug!("OKX subscription confirmed: {}", text);
                return;
            },
            "error" => {
                error!("OKX: ERROR - {}", text);
                error!("OKX error response: {}", text);
                return;
            },
            _ => {
                debug!("OKX event: {}", event);
                return;
            }
        }
    }

    // Process market data - OKX sends data in "data" array with "arg" for channel info
    if let (Some(arg), Some(data)) = (value.get("arg"), value.get("data")) {
        // Debug: log that we received market data
        debug!("OKX: Processing market data for channel");
        if let Some(channel) = arg.get("channel").and_then(|v| v.as_str()) {
            let inst_id = arg.get("instId").and_then(|v| v.as_str()).unwrap_or("");

            if inst_id.is_empty() {
                debug!("WARNING: Empty instId in OKX message");
                return;
            }

            // Map the symbol
            let common_symbol = match symbol_mapper.map("OKX", inst_id) {
                Some(symbol) => symbol,
                None => {
                    // Use original symbol if no mapping exists
                    inst_id.to_string()
                }
            };

            match channel {
                "trades" => {
                    // OKX sends trades in data array
                    if let Some(trades_array) = data.as_array() {
                        debug!("OKX: Receiving trade data for {}", common_symbol);
                        for trade_data in trades_array {
                            // HFT: Get precision from config
                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            // HFT: Parse directly to scaled i64 (NO f64!)
                            // OKX trade fields: px (price), sz (size), ts (timestamp)
                            let price = trade_data.get("px")
                                .and_then(|v| v.as_str())
                                .map(|s| parse_to_scaled_or_default(s, price_precision))
                                .unwrap_or(0);
                            let quantity = trade_data.get("sz")
                                .and_then(|v| v.as_str())
                                .map(|s| parse_to_scaled_or_default(s, qty_precision))
                                .unwrap_or(0);
                            let timestamp = trade_data.get("ts")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<i64>().ok())
                                .map(|t| t.max(0) as u64)
                                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);

                            let trade = TradeData {
                                exchange: "OKX".to_string(),
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
                                let mut trades = TRADES.write();
                                trades.push(trade.clone());
                            }

                            // Send UDP packet using multi-port sender if enabled, fallback to single-port
                            if USE_MULTI_PORT_UDP {
                                if let Some(sender) = get_multi_port_sender() {
                                    let _ = sender.send_trade_data(trade.clone());
                                } else {
                                    // Fallback to single-port
                                    if let Some(sender) = crate::core::get_binary_udp_sender() {
                                        let _ = sender.send_trade_data(trade.clone());
                                    }
                                }
                            } else {
                                // Use original single-port sender
                                if let Some(sender) = crate::core::get_binary_udp_sender() {
                                    let _ = sender.send_trade_data(trade.clone());
                                }
                            }
                            debug!("OKX: Sent UDP packet for {} trade at price {}", trade.symbol, trade.price);

                            COMPARE_NOTIFY.notify_waiters();
                        }
                    }
                },
                "books5" | "books" | "books50-l2-tbt" => {
                    // OKX sends orderbook updates in data array
                    if let Some(books_array) = data.as_array() {
                        debug!("OKX: Receiving orderbook data for {}", common_symbol);
                        for book_data in books_array {
                            // HFT: Get precision BEFORE parsing orderbook
                            let price_precision = config.feed_config.get_price_precision(&common_symbol);
                            let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

                            // HFT: Parse directly to scaled i64 (NO f64!)
                            let bids: Vec<(i64, i64)> = book_data.get("bids")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter().filter_map(|level| {
                                        if let Some(level_arr) = level.as_array() {
                                            if level_arr.len() >= 2 {
                                                let price_str = level_arr[0].as_str()?;
                                                let qty_str = level_arr[1].as_str()?;
                                                let price = parse_to_scaled_or_default(price_str, price_precision);
                                                let qty = parse_to_scaled_or_default(qty_str, qty_precision);
                                                Some((price, qty))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    }).collect()
                                })
                                .unwrap_or_default();

                            let asks: Vec<(i64, i64)> = book_data.get("asks")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter().filter_map(|level| {
                                        if let Some(level_arr) = level.as_array() {
                                            if level_arr.len() >= 2 {
                                                let price_str = level_arr[0].as_str()?;
                                                let qty_str = level_arr[1].as_str()?;
                                                let price = parse_to_scaled_or_default(price_str, price_precision);
                                                let qty = parse_to_scaled_or_default(qty_str, qty_precision);
                                                Some((price, qty))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    }).collect()
                                })
                                .unwrap_or_default();

                            let timestamp = book_data.get("ts")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<i64>().ok())
                                .map(|t| t.max(0) as u64)
                                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);

                            let orderbook = OrderBookData {
                                exchange: "OKX".to_string(),
                                symbol: common_symbol.clone(),
                                asset_type: asset_type.to_string(),
                                bids,
                                asks,
                                price_precision,
                                quantity_precision: qty_precision,
                                timestamp,
                                timestamp_unit: config.feed_config.timestamp_unit,
                            };

                            {
                                let mut orderbooks = ORDERBOOKS.write();
                                orderbooks.push(orderbook.clone());
                            }

                            // Send UDP packet using multi-port sender if enabled, fallback to single-port
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
                            debug!("OKX: Sent UDP packet for {} orderbook with {} bids, {} asks",
                                orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());

                            COMPARE_NOTIFY.notify_waiters();
                        }
                    }
                },
                _ => {
                    debug!("Unknown OKX channel: {}", channel);
                }
            }
        }
    }
}