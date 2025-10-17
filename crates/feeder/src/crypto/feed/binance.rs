use async_trait::async_trait;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tracing::{debug, info, warn, error};
use super::websocket_config::connect_with_large_buffer;
use super::binance_orderbook::{process_binance_orderbook, is_orderbook_message};
use super::binance_snapshot::{BinanceOrderbookSynchronizer, BufferedDepthEvent};
use chrono;
use tokio::sync::RwLock;

use crate::core::{
    Feeder, TRADES, ORDERBOOKS, COMPARE_NOTIFY, TradeData, OrderBookData,
    SymbolMapper, get_shutdown_receiver, CONNECTION_STATS,
    // NEW: Multi-port UDP sender
    MultiPortUdpSender, get_multi_port_sender,
    // HFT: Direct string → scaled i64 parsing (NO f64!)
    parse_to_scaled_or_default,
};
use crate::core::robust_connection::ExchangeConnectionLimits;
use crate::error::{Result, Error};
use crate::load_config::ExchangeConfig;
use crate::connect_to_databse::ConnectionEvent;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// FEATURE FLAG: Set to true to enable multi-port UDP (can disable for rollback)
const USE_MULTI_PORT_UDP: bool = true;

pub struct BinanceExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    // Map of (asset_type, stream_type, chunk_index) -> WebSocket connection
    // Each connection handles up to 10 symbols for either trade OR orderbook
    ws_streams: HashMap<(String, String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    // Track active connections for proper reconnection
    active_connections: Arc<AtomicUsize>,

    // NEW: Multi-port UDP sender (optional)
    multi_port_sender: Option<Arc<MultiPortUdpSender>>,

    // NEW: Orderbook synchronizers (symbol -> synchronizer)
    // Each symbol needs its own synchronizer to handle REST snapshot + WebSocket sync
    orderbook_synchronizers: Arc<RwLock<HashMap<String, Arc<BinanceOrderbookSynchronizer>>>>,
}

impl BinanceExchange {
    fn ensure_sender(&mut self) {
        if self.multi_port_sender.is_none() {
            if let Some(sender) = get_multi_port_sender() {
                self.multi_port_sender = Some(sender);
                info!("BinanceExchange: multi_port_sender initialized");
            }
        }
    }

    pub fn new(
        config: ExchangeConfig,
        symbol_mapper: Arc<SymbolMapper>,
    ) -> Self {
        const SYMBOLS_PER_CONNECTION: usize = 10;
        let mut ws_streams = HashMap::new();

        // Initialize separate connections for trade and orderbook streams
        for asset_type in &config.feed_config.asset_type {
            let symbols = if asset_type == "spot" {
                &config.subscribe_data.spot_symbols
            } else {
                &config.subscribe_data.futures_symbols
            };

            let num_symbols = symbols.len();
            let num_chunks = (num_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;

            for stream_type in &["trade", "orderbook"] {
                for chunk_idx in 0..num_chunks {
                    ws_streams.insert(
                        (asset_type.clone(), stream_type.to_string(), chunk_idx),
                        None
                    );
                }
            }
        }

        // Multi-port sender will be initialized in ensure_sender()
        let multi_port_sender = None;

        // Initialize empty synchronizers map (will be populated after connection)
        let orderbook_synchronizers = Arc::new(RwLock::new(HashMap::new()));

        Self {
            config,
            symbol_mapper,
            ws_streams,
            active_connections: Arc::new(AtomicUsize::new(0)),
            multi_port_sender,
            orderbook_synchronizers,
        }
    }

    /// Initialize orderbook synchronizers for all symbols and fetch REST snapshots
    async fn initialize_orderbook_synchronizers(&mut self) -> Result<()> {
        info!("Binance: Initializing orderbook synchronizers and fetching REST snapshots...");

        let mut all_symbols = Vec::new();

        // Collect all symbols from all asset types
        for asset_type in &self.config.feed_config.asset_type {
            let symbols = if asset_type == "spot" {
                &self.config.subscribe_data.spot_symbols
            } else {
                &self.config.subscribe_data.futures_symbols
            };

            for symbol in symbols {
                all_symbols.push((symbol.clone(), asset_type.clone()));
            }
        }

        info!("Binance: Creating synchronizers for {} symbols", all_symbols.len());

        // Create synchronizers for each symbol
        let mut sync_tasks = Vec::new();
        let synchronizers = self.orderbook_synchronizers.clone();

        for (symbol, asset_type) in all_symbols {
            let synchronizer = Arc::new(BinanceOrderbookSynchronizer::new(
                symbol.clone(),
                asset_type.clone(),
            ));

            // Store synchronizer
            {
                let mut sync_map = synchronizers.write().await;
                sync_map.insert(symbol.clone(), synchronizer.clone());
            }

            // Spawn task to fetch snapshot
            let sync_clone = synchronizer.clone();
            let symbol_clone = symbol.clone();
            sync_tasks.push(tokio::spawn(async move {
                match sync_clone.fetch_snapshot().await {
                    Ok(_) => {
                        info!("✅ Binance: Snapshot fetched for {}", symbol_clone);
                        Ok(())
                    }
                    Err(e) => {
                        error!("❌ Binance: Failed to fetch snapshot for {}: {}", symbol_clone, e);
                        Err(e)
                    }
                }
            }));
        }

        // Wait for all snapshots to be fetched (with timeout)
        let timeout_duration = Duration::from_secs(30);
        match tokio::time::timeout(timeout_duration, futures::future::join_all(sync_tasks)).await {
            Ok(results) => {
                let mut success_count = 0;
                let mut failure_count = 0;

                for result in results {
                    match result {
                        Ok(Ok(())) => success_count += 1,
                        Ok(Err(_)) | Err(_) => failure_count += 1,
                    }
                }

                info!("Binance: Snapshot initialization complete: {} succeeded, {} failed",
                    success_count, failure_count);

                if failure_count > 0 {
                    warn!("⚠️ Binance: Some snapshots failed to fetch. Orderbook sync may not work for those symbols.");
                }

                Ok(())
            }
            Err(_) => {
                error!("❌ Binance: Snapshot fetching timed out after 30 seconds");
                Err(Error::Connection("Snapshot fetch timeout".to_string()))
            }
        }
    }

    async fn connect_websocket(&mut self, asset_type: &str, stream_type: &str, chunk_idx: usize) -> Result<()> {
        const SYMBOLS_PER_CONNECTION: usize = 10;

        // Get symbols based on asset type
        let symbols = if asset_type == "spot" {
            &self.config.subscribe_data.spot_symbols
        } else {
            &self.config.subscribe_data.futures_symbols
        };

        // Get the symbols for this chunk
        let start_idx = chunk_idx * SYMBOLS_PER_CONNECTION;
        let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, symbols.len());
        let chunk_symbols = &symbols[start_idx..end_idx];

        // Build streams URL for ONLY this stream type
        let mut streams = Vec::new();
        for code in chunk_symbols {
            let lower_code = code.to_lowercase();

            if stream_type == "trade" {
                // Add only trade stream
                streams.push(format!("{}@trade", lower_code));
            } else {
                // Add only orderbook stream
                if self.config.subscribe_data.order_depth > 0 {
                    // Binance only supports depth5, depth10, depth20 for partial book
                    // For 50 levels, we use depth20 and will get 20 levels
                    let depth_level = if self.config.subscribe_data.order_depth <= 5 {
                        5
                    } else if self.config.subscribe_data.order_depth <= 10 {
                        10
                    } else {
                        20  // Maximum partial book depth supported by Binance
                    };
                    // Use 0ms for futures, 100ms for spot
                    let update_speed = if asset_type == "futures" { "0ms" } else { "100ms" };
                    streams.push(format!("{}@depth{}@{}", lower_code, depth_level, update_speed));
                } else {
                    // Default to depth20 - 0ms for futures, 100ms for spot
                    let update_speed = if asset_type == "futures" { "0ms" } else { "100ms" };
                    streams.push(format!("{}@depth20@{}", lower_code, update_speed));
                }
            }
        }
        
        // Use combined streams endpoint - connect directly to the streams we want
        let stream_param = streams.join("/");
        let ws_url = match asset_type {
            "spot" => format!("wss://stream.binance.com:9443/stream?streams={}", stream_param),
            "futures" => format!("wss://fstream.binance.com/stream?streams={}", stream_param),
            _ => return Err(Error::InvalidInput(format!("Invalid asset type: {}", asset_type))),
        };

        debug!("Attempting to connect to Binance {} stream for {} chunk {}",
            stream_type, asset_type, chunk_idx);
        debug!("  Symbols in this connection: {:?}", chunk_symbols);
        debug!("  Streams subscribed: {:?}", streams);
        debug!("  URL length: {} chars", ws_url.len());

        match connect_with_large_buffer(&ws_url).await {
            Ok((ws_stream, response)) => {
                debug!("Binance WebSocket connected for {} {} chunk {}", asset_type, stream_type, chunk_idx);
                debug!("Binance Connection response: {:?}", response);

                // Update connection stats - mark as connected
                {
                    let mut stats = CONNECTION_STATS.write();
                    let key = format!("Binance_{}_{}", asset_type, stream_type);
                    let entry = stats.entry(key.clone()).or_default();
                    entry.connected += 1;
                    entry.total_connections += 1;
                    info!("[{}] Connection established for chunk {}", key, chunk_idx);
                }

                // Log successful connection to QuestDB
                let connection_id = format!("binance-{}-{}-{}", asset_type, stream_type, chunk_idx);
                let _ = crate::core::feeder_metrics::log_connection(
                    "Binance",
                    &connection_id,
                    ConnectionEvent::Connected
                ).await;

                if let Some(stream_slot) = self.ws_streams.get_mut(&(asset_type.to_string(), stream_type.to_string(), chunk_idx)) {
                    *stream_slot = Some(ws_stream);
                }
                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to Binance WebSocket for {} {} chunk {}: {}",
                    asset_type, stream_type, chunk_idx, e);
                Err(Error::Connection(format!("Binance connection failed: {}", e)))
            }
        }
    }

}


#[async_trait]
impl Feeder for BinanceExchange {
    fn name(&self) -> &str {
        "Binance"
    }   

    async fn connect(&mut self) -> Result<()> {
        let mut retry_delay = Duration::from_secs(self.config.connect_config.initial_retry_delay_secs);
        let connection_keys: Vec<(String, String, usize)> = self.ws_streams
            .keys()
            .map(|(asset, stream, chunk)| (asset.clone(), stream.clone(), *chunk))
            .collect();

        info!("Binance: Creating {} WebSocket connections (10 symbols each, separate for trade and orderbook)", connection_keys.len());

        for (asset_type, stream_type, chunk_idx) in connection_keys {
            loop {
                match self.connect_websocket(&asset_type, &stream_type, chunk_idx).await {
                    Ok(()) => {
                        debug!("Successfully connected to Binance {} {} chunk {}",
                            asset_type, stream_type, chunk_idx);
                        retry_delay = Duration::from_secs(self.config.connect_config.initial_retry_delay_secs);  // Reset delay on success
                        break;  // Move to next connection
                    },
                    Err(e) => {
                        error!("Failed to connect to Binance {} {} chunk {}: {}",
                            asset_type, stream_type, chunk_idx, e);
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
        // When using combined streams endpoint, we don't need to send subscription messages
        // The streams are already specified in the connection URL
        debug!("Using combined streams - no subscription message needed");

        // Initialize orderbook synchronizers and fetch REST snapshots
        self.initialize_orderbook_synchronizers().await?;

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        // Ensure we have a sender before starting
        self.ensure_sender();

        // Reset connection counter
        self.active_connections.store(0, Ordering::SeqCst);

        for ((asset_type, stream_type, chunk_idx), ws_stream_opt) in self.ws_streams.iter_mut() {
            if let Some(mut ws_stream) = ws_stream_opt.take() {
                let asset_type = asset_type.clone();
                let stream_type = stream_type.clone();
                let chunk_idx = *chunk_idx;
                let symbol_mapper = self.symbol_mapper.clone();
                let active_connections = self.active_connections.clone();
                let multi_port_sender = self.multi_port_sender.clone();
                let config = self.config.clone();
                let synchronizers = self.orderbook_synchronizers.clone();

                // Increment active connection count
                active_connections.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    debug!("Starting Binance {} {} chunk {} stream processing",
                        asset_type, stream_type, chunk_idx);
                    
                    // Get shutdown receiver
                    let mut shutdown_rx = get_shutdown_receiver();
                    
                    // Connection tracking is handled in feeder.rs, not here
                    
                    // Add periodic ping to keep connection alive
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
                    
                    loop {
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() {
                                    info!("Binance {} {} chunk {} received shutdown signal", asset_type, stream_type, chunk_idx);
                                    break;
                                }
                            }
                            _ = ping_interval.tick() => {
                                // Send ping to keep connection alive
                                if let Err(e) = ws_stream.send(Message::Ping(vec![].into())).await {
                                    warn!("Failed to send ping to Binance {} {} chunk {}: {}",
                                        asset_type, stream_type, chunk_idx, e);
                                    error!("Connection lost for Binance {} {} chunk {}",
                                        asset_type, stream_type, chunk_idx);

                                    // Log disconnection to QuestDB
                                    let connection_id = format!("binance-{}-{}-{}", asset_type, stream_type, chunk_idx);
                                    let _ = crate::core::feeder_metrics::log_connection(
                                        "Binance",
                                        &connection_id,
                                        ConnectionEvent::Disconnected {
                                            reason: "ping failed".to_string()
                                        }
                                    ).await;
                                    error!("[Binance_{}_{}] WebSocket disconnected for chunk {} - ping failed", asset_type, stream_type, chunk_idx);
                                    break;
                                }
                                debug!("Sent ping to Binance {} {} chunk {}",
                                    asset_type, stream_type, chunk_idx);
                            }
                            msg = ws_stream.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        // Add debug to see raw messages for orderbooks
                                        if text.contains("depth") {
                                            debug!("Binance depth message preview: {}", 
                                                if text.len() > 200 { &text[..200] } else { &text });
                                        }
                                        process_binance_message(&text, symbol_mapper.clone(), &asset_type, multi_port_sender.clone(), &config, synchronizers.clone()).await;
                                    },
                                    Some(Ok(Message::Ping(ping))) => {
                                        if let Err(e) = ws_stream.send(Message::Pong(ping)).await {
                                            debug!("Failed to send pong: {}", e);
                                            break;
                                        }
                                    },
                                    Some(Ok(Message::Pong(_))) => {
                                        debug!("Received pong from Binance {} {} chunk {}",
                                            asset_type, stream_type, chunk_idx);
                                    },
                                    Some(Ok(Message::Close(frame))) => {
                                        warn!("WebSocket closed by Binance server: {:?}", frame);

                                        // Connection tracking is handled in feeder.rs
                                        // Log disconnection/error to file
                                        error!("[Binance_{}_{}] WebSocket issue for chunk {}", asset_type, stream_type, chunk_idx);
                                        break;
                                    },
                                    Some(Ok(_)) => {
                                        // Handle other message types if needed
                                    },
                                    Some(Err(e)) => {
                                        let error_str = e.to_string();
                                        if error_str.contains("10054") || error_str.contains("forcibly closed") || error_str.contains("강제로 끊겼습니다") {
                                            warn!("Binance {} {} chunk {} connection forcibly closed by remote host (error 10054)", asset_type, stream_type, chunk_idx);
                                        } else {
                                            error!("WebSocket error for Binance {} {} chunk {}: {}",
                                                asset_type, stream_type, chunk_idx, e);
                                        }

                                        // Connection tracking is handled in feeder.rs
                                        // Log disconnection/error to file
                                        error!("[Binance_{}_{}] WebSocket issue for chunk {}", asset_type, stream_type, chunk_idx);
                                        break;
                                    },
                                    None => {
                                        warn!("WebSocket stream ended for Binance {} {} chunk {}",
                                            asset_type, stream_type, chunk_idx);

                                        // Connection tracking is handled in feeder.rs
                                        // Log disconnection/error to file
                                        error!("[Binance_{}_{}] WebSocket issue for chunk {}", asset_type, stream_type, chunk_idx);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    warn!("Binance {} {} chunk {} disconnected. Connection will be retried by feeder.",
                        asset_type, stream_type, chunk_idx);
                    
                    // Decrement active connection count
                    active_connections.fetch_sub(1, Ordering::SeqCst);
                });
            }
        }

        // Wait until all connections are disconnected instead of infinite loop
        while self.active_connections.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        warn!("All Binance connections lost, returning to trigger reconnection");
        Ok(())
    }
}

async fn process_binance_message(
    text: &str,
    symbol_mapper: Arc<SymbolMapper>,
    asset_type: &str,
    multi_port_sender: Option<Arc<MultiPortUdpSender>>,
    config: &ExchangeConfig,
    synchronizers: Arc<RwLock<HashMap<String, Arc<BinanceOrderbookSynchronizer>>>>,
) {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", crate::core::safe_json_parse_error_log(&e, text));
            return;
        }
    };

    // Debug block removed for production performance

    // Handle subscription responses (these don't have trade/orderbook data)
    if value.get("result").is_some() && value.get("id").is_some() {
        // This is a subscription confirmation, not market data
        return;
    }

    // Extract stream name to determine the message type
    let stream_name = value.get("stream").and_then(|v| v.as_str()).unwrap_or("");
    
    // Debug log to see what streams we're receiving  
    if !stream_name.is_empty() {
        if !stream_name.contains("trade") && !stream_name.contains("depth") {
            debug!("Binance unexpected stream: {}", stream_name);
        }
        // Log all depth streams to verify format
        if stream_name.contains("depth") {
            debug!("Binance depth stream received: {}", stream_name);
        }
    }
    
    // Binance combined streams send data wrapped in a "data" field with "stream" name
    let actual_data = if value.get("stream").is_some() && value.get("data").is_some() {
        // Combined stream format: {"stream":"btcusdt@trade","data":{actual data}}
        &value["data"]
    } else {
        // Single stream format: {actual data}
        &value
    };

    // Determine message type from stream name
    let stream_type = if stream_name.contains("@trade") {
        "trade"
    } else if stream_name.contains("@depth") {
        "orderbook"
    } else {
        // Try to infer from data structure
        if actual_data.get("e").and_then(|v| v.as_str()) == Some("trade") {
            "trade"
        } else if (actual_data.get("b").is_some() && actual_data.get("a").is_some()) ||
                  (actual_data.get("bids").is_some() && actual_data.get("asks").is_some()) {
            "orderbook"
        } else {
            debug!("Unknown message type for stream: {}", stream_name);
            return;
        }
    };

    match stream_type {
        "trade" => {
            // Get the original symbol - Binance uses lowercase "s" for symbol in trade messages
            let original_symbol = actual_data.get("s")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            
            if original_symbol.is_empty() {
                debug!("WARNING: Empty symbol in Binance trade message. Full data: {:?}", actual_data);
                return;
            }
            
            let common_symbol = match symbol_mapper.map("Binance", original_symbol) {
                Some(symbol) => symbol,
                None => {
                    // Log missing mapping with actual symbol name
                    // Silent - this is expected for some symbols
                    return;
                }
            };

            // println!("Binance: Receiving trade data for {}", common_symbol);

            // HFT: Get precision from config (default to 8)
            let price_precision = config.feed_config.get_price_precision(&common_symbol);
            let quantity_precision = config.feed_config.get_quantity_precision(&common_symbol);

            // HFT: Parse directly to scaled i64 (NO f64!)
            let price = actual_data["p"].as_str()
                .map(|s| parse_to_scaled_or_default(s, price_precision))
                .unwrap_or(0);

            let quantity = actual_data["q"].as_str()
                .map(|s| parse_to_scaled_or_default(s, quantity_precision))
                .unwrap_or(0);

            let trade = TradeData {
                exchange: "Binance".to_string(),
                symbol: common_symbol,  // Use mapped symbol
                asset_type: asset_type.to_string(),
                price,
                quantity,
                price_precision,
                quantity_precision,
                timestamp: actual_data["T"]
                    .as_i64()
                    .and_then(|t| if t > 0 { Some(t as u64) } else { None })
                    .map(|t| {
                        // Validate and fix timestamp
                        let (is_valid, normalized) = crate::core::process_timestamp_field(t, "Binance");
                        if !is_valid {
                            // T field contains sequence number, use current time
                            chrono::Utc::now().timestamp_millis() as u64
                        } else {
                            normalized
                        }
                    })
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64),
                timestamp_unit: config.feed_config.timestamp_unit,
            };

            {
                let mut trades = TRADES.write();
                trades.push(trade.clone());
            }

            // Send UDP packet using multi-port sender if enabled, fallback to single-port
            if USE_MULTI_PORT_UDP {
                if let Some(sender) = &multi_port_sender {
                    let _ = sender.send_trade_data(trade.clone());
                    // println!("Binance: Sent multi-port UDP for {} trade at price {}", trade.symbol, trade.price);
                } else {
                    // Fallback to single-port
                    if let Some(sender) = crate::core::get_binary_udp_sender() {
                        let _ = sender.send_trade_data(trade.clone());
                        // println!("Binance: Sent single-port UDP for {} trade at price {}", trade.symbol, trade.price);
                    }
                }
            } else {
                // Use original single-port sender
                if let Some(sender) = crate::core::get_binary_udp_sender() {
                    let _ = sender.send_trade_data(trade.clone());
                    // println!("Binance: Sent UDP packet for {} trade at price {}", trade.symbol, trade.price);
                }
            }

            COMPARE_NOTIFY.notify_waiters();
        },
        "orderbook" => {
            // NEW: Binance orderbook synchronization with REST snapshot
            // Extract symbol from stream name (format: "btcusdt@depth20@100ms")
            let original_symbol = if !stream_name.is_empty() {
                stream_name.split('@').next().unwrap_or("").to_uppercase()
            } else {
                actual_data.get("s")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };

            if original_symbol.is_empty() {
                debug!("Cannot determine symbol for Binance orderbook message");
                return;
            }

            // Extract U/u/pu sequence fields
            let final_update_id = actual_data.get("u")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let first_update_id = actual_data.get("U")
                .and_then(|v| v.as_u64())
                .unwrap_or(final_update_id);
            let prev_update_id = actual_data.get("pu")
                .and_then(|v| v.as_u64());

            // Get synchronizer for this symbol
            let sync_map = synchronizers.read().await;
            let synchronizer = match sync_map.get(&original_symbol) {
                Some(sync) => sync.clone(),
                None => {
                    // No synchronizer = not subscribed to this symbol
                    debug!("No synchronizer for Binance symbol {}", original_symbol);
                    return;
                }
            };
            drop(sync_map);

            // Check if synchronization is needed
            if synchronizer.needs_sync().await {
                // Buffer this event
                let event = BufferedDepthEvent {
                    first_update_id,
                    final_update_id,
                    prev_update_id,
                    data: actual_data.clone(),
                };
                synchronizer.buffer_event(event).await;

                // Try to synchronize
                match synchronizer.synchronize().await {
                    Ok(validated_events) => {
                        info!("✅ Binance {} synchronized: {} buffered events ready",
                            original_symbol, validated_events.len());

                        // Process all validated events
                        for event_data in validated_events {
                            process_binance_orderbook_data(
                                &event_data,
                                &original_symbol,
                                &symbol_mapper,
                                asset_type,
                                &multi_port_sender,
                                config,
                            );
                        }
                    }
                    Err(e) => {
                        // Synchronization failed - will retry on next message
                        debug!("Binance {} sync pending: {}", original_symbol, e);
                        return;
                    }
                }
            } else {
                // Already synchronized - validate sequence
                let event = BufferedDepthEvent {
                    first_update_id,
                    final_update_id,
                    prev_update_id,
                    data: actual_data.clone(),
                };

                if synchronizer.validate_event(&event).await {
                    // Sequence valid - process normally
                    process_binance_orderbook_data(
                        actual_data,
                        &original_symbol,
                        &symbol_mapper,
                        asset_type,
                        &multi_port_sender,
                        config,
                    );
                } else {
                    warn!("❌ Binance {} sequence gap detected - resyncing", original_symbol);
                    // Buffer and resync
                    synchronizer.buffer_event(event).await;
                }
            }
        },
        _ => {
            debug!("Received unknown Binance stream type: {}", stream_type);
        }
    }
}

/// Process Binance orderbook data (after synchronization)
fn process_binance_orderbook_data(
    actual_data: &Value,
    original_symbol: &str,
    symbol_mapper: &Arc<SymbolMapper>,
    asset_type: &str,
    multi_port_sender: &Option<Arc<MultiPortUdpSender>>,
    config: &ExchangeConfig,
) {
    // Send to HFT OrderBook processor for ultra-fast local orderbook
    crate::core::integrate_binance_orderbook(actual_data, original_symbol, "");

    let common_symbol = match symbol_mapper.map("Binance", original_symbol) {
        Some(symbol) => symbol,
        None => {
            // Symbol not mapped, skip silently
            return;
        }
    };

    // Try both formats: "b"/"a" for full depth, "bids"/"asks" for partial depth
    let bids = actual_data.get("b")
        .or_else(|| actual_data.get("bids"))
        .and_then(|v| v.as_array());
    let asks = actual_data.get("a")
        .or_else(|| actual_data.get("asks"))
        .and_then(|v| v.as_array());

    if let Some(bids) = bids {
        // HFT: Get precision from config
        let price_precision = config.feed_config.get_price_precision(&common_symbol);
        let qty_precision = config.feed_config.get_quantity_precision(&common_symbol);

        let orderbook = OrderBookData {
            exchange: "Binance".to_string(),
            symbol: common_symbol.clone(),
            asset_type: asset_type.to_string(),
            bids: bids.iter()
                .filter_map(|bid| {
                    // HFT: Parse directly to scaled i64 (NO f64!)
                    let price = parse_to_scaled_or_default(bid[0].as_str()?, price_precision);
                    let qty = parse_to_scaled_or_default(bid[1].as_str()?, qty_precision);
                    Some((price, qty))
                })
                .collect(),
            asks: asks
                .unwrap_or(&Vec::new())
                .iter()
                .filter_map(|ask| {
                    // HFT: Parse directly to scaled i64 (NO f64!)
                    let price = parse_to_scaled_or_default(ask[0].as_str()?, price_precision);
                    let qty = parse_to_scaled_or_default(ask[1].as_str()?, qty_precision);
                    Some((price, qty))
                })
                .collect(),
            price_precision,
            quantity_precision: qty_precision,
            timestamp: actual_data.get("E")  // Event time
                .and_then(|v| v.as_i64())
                .map(|t| t.max(0) as u64)
                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64),
            timestamp_unit: config.feed_config.timestamp_unit,
        };

        {
            let mut orderbooks = ORDERBOOKS.write();
            orderbooks.push(orderbook.clone());
        }

        // Send UDP packet
        if USE_MULTI_PORT_UDP {
            if let Some(sender) = multi_port_sender {
                let _ = sender.send_orderbook_data(orderbook.clone());
            } else {
                if let Some(sender) = crate::core::get_binary_udp_sender() {
                    let _ = sender.send_orderbook_data(orderbook.clone());
                }
            }
        } else {
            if let Some(sender) = crate::core::get_binary_udp_sender() {
                let _ = sender.send_orderbook_data(orderbook.clone());
            }
        }

        COMPARE_NOTIFY.notify_waiters();
    }
}