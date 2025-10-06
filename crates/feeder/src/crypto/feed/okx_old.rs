use async_trait::async_trait;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use super::websocket_config::connect_with_large_buffer;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn, error};
use anyhow::anyhow;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::{OrderBookData, TradeData, Feeder, SymbolMapper, CONNECTION_STATS, get_shutdown_receiver, TRADES, ORDERBOOKS};
use std::collections::BTreeMap;
use crc32fast::Hasher;

/// OKX Orderbook manager for handling incremental updates
#[derive(Debug, Clone)]
struct OkxOrderbook {
    symbol: String,
    bids: BTreeMap<String, f64>,  // price -> quantity
    asks: BTreeMap<String, f64>,  // price -> quantity
    last_update_id: Option<i64>,
    checksum: Option<u32>,
}

impl OkxOrderbook {
    fn new(symbol: String) -> Self {
        Self {
            symbol,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id: None,
            checksum: None,
        }
    }

    /// Process snapshot - replace entire orderbook
    fn process_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        self.bids.clear();
        self.asks.clear();

        for (price, qty) in bids {
            if *qty > 0.0 {
                self.bids.insert(format!("{}", price), *qty);
            }
        }

        for (price, qty) in asks {
            if *qty > 0.0 {
                self.asks.insert(format!("{}", price), *qty);
            }
        }
    }

    /// Process incremental update
    fn process_update(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        // Update bids
        for (price, qty) in bids {
            let price_str = format!("{}", price);
            if *qty == 0.0 {
                // Remove level if quantity is 0
                self.bids.remove(&price_str);
            } else {
                // Add or update level
                self.bids.insert(price_str, *qty);
            }
        }

        // Update asks
        for (price, qty) in asks {
            let price_str = format!("{}", price);
            if *qty == 0.0 {
                // Remove level if quantity is 0
                self.asks.remove(&price_str);
            } else {
                // Add or update level
                self.asks.insert(price_str, *qty);
            }
        }
    }

    /// Calculate CRC32 checksum for validation (OKX format)
    fn calculate_checksum(&self) -> u32 {
        let mut checksum_str = Vec::new();

        // Get top 25 bids (descending) and asks (ascending)
        let bids: Vec<_> = self.bids.iter().rev().take(25).collect();
        let asks: Vec<_> = self.asks.iter().take(25).collect();

        // Interleave bids and asks as per OKX specification
        for i in 0..25 {
            if i < bids.len() {
                let (price, qty) = bids[i];
                checksum_str.push(format!("{}:{}", price, qty));
            }
            if i < asks.len() {
                let (price, qty) = asks[i];
                checksum_str.push(format!("{}:{}", price, qty));
            }
        }

        // Join with colon and calculate CRC32
        let checksum_string = checksum_str.join(":");
        let mut hasher = Hasher::new();
        hasher.update(checksum_string.as_bytes());
        hasher.finalize()
    }

    /// Validate checksum against exchange-provided value
    fn validate_checksum(&self, exchange_checksum: i32) -> bool {
        let calculated = self.calculate_checksum();
        // OKX sends signed int32, we need unsigned comparison
        let exchange_unsigned = (exchange_checksum as u32) & 0xFFFFFFFF;
        calculated == exchange_unsigned
    }

    /// Get top N levels for display/processing
    fn get_top_levels(&self, n: usize) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
        let bids: Vec<(f64, f64)> = self.bids
            .iter()
            .rev()
            .take(n)
            .map(|(p, q)| (p.parse::<f64>().unwrap_or(0.0), *q))
            .collect();

        let asks: Vec<(f64, f64)> = self.asks
            .iter()
            .take(n)
            .map(|(p, q)| (p.parse::<f64>().unwrap_or(0.0), *q))
            .collect();

        (bids, asks)
    }
}
use crate::error::Result;
use crate::load_config::ExchangeConfig;

pub struct OkxExchange {
    config: ExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>,
    ws_streams: HashMap<(String, usize), Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>,
    active_connections: Arc<AtomicUsize>,
    orderbooks: Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>,
    orderbook_managers: Arc<parking_lot::RwLock<HashMap<String, OkxOrderbook>>>,  // For incremental updates
    ws_subscriptions: HashMap<(String, usize), Vec<String>>,  // Track subscriptions for reconnect
    subscription_messages: HashMap<String, String>,  // Store subscription messages for reconnect
}

impl OkxExchange {
    pub fn new(config: ExchangeConfig, symbol_mapper: Arc<SymbolMapper>) -> Self {
        const SYMBOLS_PER_CONNECTION: usize = 5;
        let mut ws_streams = HashMap::new();

        // Count symbols per asset type
        for asset_type in &config.feed_config.asset_type {
            let num_symbols = match asset_type.as_str() {
                "spot" => config.subscribe_data.spot_symbols.len(),
                "swap" => config.subscribe_data.futures_symbols.len(),
                _ => 0,
            };

            let num_chunks = (num_symbols + SYMBOLS_PER_CONNECTION - 1) / SYMBOLS_PER_CONNECTION;

            println!("OKX: {} will use {} chunks for {} symbols",
                     asset_type, num_chunks, num_symbols);

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
            orderbook_managers: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            ws_subscriptions: HashMap::new(),
            subscription_messages: HashMap::new(),
        }
    }
}

#[async_trait]
impl Feeder for OkxExchange {
    fn name(&self) -> &str {
        "OKX"
    }

    async fn connect(&mut self) -> Result<()> {
        let config = self.config.clone();

        info!("OKX: Creating {} WebSocket connections (5 symbols each, with both trade and orderbook streams)", self.ws_streams.len());

        for ((asset_type, chunk_idx), ws_stream) in self.ws_streams.iter_mut() {
            let ws_url = "wss://ws.okx.com:8443/ws/v5/public";
            debug!("Connecting to OKX {} chunk {}: {}", asset_type, chunk_idx, ws_url);

            let mut retry_count = 0;
            let max_retries = 5;
            let mut retry_delay = Duration::from_secs(config.connect_config.initial_retry_delay_secs);

            loop {
                match connect_with_large_buffer(ws_url).await {
                    Ok((stream, _)) => {
                        *ws_stream = Some(stream);
                        info!("Successfully connected to OKX {} chunk {} at {}",
                            asset_type, chunk_idx, ws_url);

                        // Update connection stats - mark as connected
                        CONNECTION_STATS.write()
                            .entry("okx".to_string())
                            .and_modify(|e| e.connected += 1)
                            .or_insert_with(|| crate::core::ConnectionStats::default());

                        self.active_connections.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                    Err(e) => {
                        retry_count += 1;
                        error!("Failed to connect to OKX {} chunk {}: {}. Retry {}/{}",
                            asset_type, chunk_idx, e, retry_count, max_retries);

                        if retry_count >= max_retries {
                            error!("Max retries reached for OKX {} chunk {}. Skipping this connection.",
                                asset_type, chunk_idx);

                            // Update connection stats - mark as disconnected
                            CONNECTION_STATS.write()
                                .entry("okx".to_string())
                                .and_modify(|e| e.disconnected += 1)
                                .or_insert_with(|| crate::core::ConnectionStats::default());

                            warn!("Exceeded max retries ({}) for OKX {} chunk {}", max_retries,
                                asset_type, chunk_idx);
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
                // Get symbols by asset type
                let filtered_symbols: Vec<String> = match asset_type.as_str() {
                    "spot" => config.subscribe_data.spot_symbols.clone(),
                    "swap" => config.subscribe_data.futures_symbols.clone(),
                    _ => Vec::new(),
                };

                let start_idx = *chunk_idx * SYMBOLS_PER_CONNECTION;
                let end_idx = std::cmp::min(start_idx + SYMBOLS_PER_CONNECTION, filtered_symbols.len());
                let chunk_symbols = &filtered_symbols[start_idx..end_idx];

                let mut args = Vec::new();
                for symbol in chunk_symbols {
                    // Determine instrument type based on asset_type
                    let inst_type = match asset_type.as_str() {
                        "spot" => "SPOT",
                        "swap" => "SWAP",
                        _ => "SPOT"
                    };

                    args.push(json!({
                        "channel": "trades",
                        "instType": inst_type,
                        "instId": symbol.clone()
                    }));
                    args.push(json!({
                        "channel": "books",
                        "instType": inst_type,
                        "instId": symbol.clone()
                    }));
                }

                let subscribe_msg = json!({
                    "id": format!("{}-{}", chunk_idx, chrono::Utc::now().timestamp_millis()),
                    "op": "subscribe",
                    "args": args
                });

                let subscription = subscribe_msg.to_string();
                debug!("OKX {} chunk {}: Subscribing with message: {}",
                    asset_type, chunk_idx, subscription);

                // Store subscription for reconnection
                self.ws_subscriptions.insert((asset_type.clone(), *chunk_idx), chunk_symbols.to_vec());
                self.subscription_messages.insert(
                    format!("{}-{}", asset_type, chunk_idx),
                    subscription.clone()
                );

                ws_stream.send(Message::Text(subscription.clone().into())).await?;
                info!("OKX {} chunk {}: Subscription sent for {} symbols",
                    asset_type, chunk_idx, chunk_symbols.len());
            }
        }

        Ok(())
    }

        async fn start(&mut self) -> Result<()> {
        self.active_connections.store(0, Ordering::SeqCst);

        // Clone shared data before iterating to avoid lifetime issues
        let symbol_mapper = self.symbol_mapper.clone();
        let active_connections = self.active_connections.clone();
        let orderbooks = self.orderbooks.clone();
        let orderbook_managers = self.orderbook_managers.clone();

        info!("OKX: Processing {} WebSocket streams", self.ws_streams.len());
        for ((asset_type, chunk_idx), ws_stream_opt) in self.ws_streams.iter_mut() {
            if let Some(mut ws_stream) = ws_stream_opt.take() {
                let mapper = symbol_mapper.clone();
                let active_conns = active_connections.clone();
                let asset_type_clone = asset_type.clone();
                let chunk_idx = *chunk_idx;
                let orderbook_storage = orderbooks.clone();
                let ob_managers = orderbook_managers.clone();

                tokio::spawn(async move {
                    info!("OKX: Starting message processing for {} chunk {}",
                        asset_type_clone, chunk_idx);
                    let mut shutdown = get_shutdown_receiver();

                    loop {
                        tokio::select! {
                            _ = shutdown.changed() => {
                                info!("OKX {} chunk {}: Received shutdown signal",
                                    asset_type_clone, chunk_idx);
                                break;
                            }
                            msg = ws_stream.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        match serde_json::from_str::<Value>(&text) {
                                            Ok(json) => {
                                                if json.get("event").is_some() {
                                                    debug!("OKX {} chunk {}: Received event: {}",
                                                        asset_type_clone, chunk_idx, text);
                                                    continue;
                                                }

                                                if let Some(data) = json.get("data") {
                                                    if let Some(channel) = json.get("arg")
                                                        .and_then(|arg| arg.get("channel"))
                                                        .and_then(|c| c.as_str())
                                                    {
                                                        let inst_id = json.get("arg")
                                                            .and_then(|arg| arg.get("instId"))
                                                            .and_then(|id| id.as_str())
                                                            .unwrap_or("");

                                                        // Use the symbol directly or map if needed
                                                        let internal_symbol = inst_id.to_string();

                                                        match channel {
                                                            "trades" => {
                                                                if let Some(trades_array) = data.as_array() {
                                                                    for trade in trades_array {
                                                                        // Process trade data
                                                                        if let (Some(price_str), Some(size_str)) =
                                                                            (trade.get("px").and_then(|v| v.as_str()),
                                                                             trade.get("sz").and_then(|v| v.as_str()))
                                                                        {
                                                                            if let (Ok(price), Ok(quantity)) =
                                                                                (price_str.parse::<f64>(), size_str.parse::<f64>())
                                                                            {
                                                                                let trade_data = TradeData {
                                                                                    exchange: "okx".to_string(),
                                                                                    symbol: internal_symbol.clone(),
                                                                                    asset_type: asset_type_clone.clone(),
                                                                                    price,
                                                                                    quantity,
                                                                                    timestamp: trade.get("ts")
                                                                                        .and_then(|v| v.as_str())
                                                                                        .and_then(|s| s.parse::<i64>().ok())
                                                                                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
                                                                                };
                                                                                TRADES.write().push(trade_data);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            },
                                                            "books" => {
                                                                // Check for action type (snapshot vs update)
                                                                let action = json.get("action")
                                                                    .and_then(|a| a.as_str())
                                                                    .unwrap_or("update");

                                                                if let Some(books_array) = data.as_array() {
                                                                    for book in books_array {
                                                                        // Get or create orderbook manager
                                                                        let mut managers = ob_managers.write();
                                                                        let ob_manager = managers.entry(internal_symbol.clone())
                                                                            .or_insert_with(|| OkxOrderbook::new(internal_symbol.clone()));

                                                                        // Parse bids and asks
                                                                        let mut bid_updates = Vec::new();
                                                                        let mut ask_updates = Vec::new();

                                                                        if let Some(bids) = book.get("bids").and_then(|b| b.as_array()) {
                                                                            for bid in bids {
                                                                                if let Some(bid_arr) = bid.as_array() {
                                                                                    if bid_arr.len() >= 2 {
                                                                                        if let (Some(price), Some(qty)) =
                                                                                            (bid_arr[0].as_str(), bid_arr[1].as_str()) {
                                                                                            let price_f64 = price.parse::<f64>().unwrap_or(0.0);
                                                                                            let qty_f64 = qty.parse::<f64>().unwrap_or(0.0);
                                                                                            bid_updates.push((price_f64, qty_f64));
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }

                                                                        if let Some(asks) = book.get("asks").and_then(|a| a.as_array()) {
                                                                            for ask in asks {
                                                                                if let Some(ask_arr) = ask.as_array() {
                                                                                    if ask_arr.len() >= 2 {
                                                                                        if let (Some(price), Some(qty)) =
                                                                                            (ask_arr[0].as_str(), ask_arr[1].as_str()) {
                                                                                            let price_f64 = price.parse::<f64>().unwrap_or(0.0);
                                                                                            let qty_f64 = qty.parse::<f64>().unwrap_or(0.0);
                                                                                            ask_updates.push((price_f64, qty_f64));
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }

                                                                        // Apply updates based on action type
                                                                        if action == "snapshot" {
                                                                            debug!("OKX {}: Processing orderbook snapshot", internal_symbol);
                                                                            ob_manager.process_snapshot(&bid_updates, &ask_updates);
                                                                        } else {
                                                                            ob_manager.process_update(&bid_updates, &ask_updates);
                                                                        }

                                                                        // Validate checksum if provided
                                                                        if let Some(checksum) = book.get("checksum").and_then(|c| c.as_i64()) {
                                                                            if !ob_manager.validate_checksum(checksum as i32) {
                                                                                error!("OKX {}: Checksum validation failed! Local: {} Exchange: {}",
                                                                                    internal_symbol,
                                                                                    ob_manager.calculate_checksum(),
                                                                                    checksum);
                                                                                // Could trigger reconnection here
                                                                            }
                                                                        }

                                                                        // Get top levels for broadcasting
                                                                        let (top_bids, top_asks) = ob_manager.get_top_levels(20);

                                                                        let orderbook_data = OrderBookData {
                                                                            exchange: "okx".to_string(),
                                                                            symbol: internal_symbol.clone(),
                                                                            asset_type: asset_type_clone.clone(),
                                                                            bids: top_bids,
                                                                            asks: top_asks,
                                                                            timestamp: book.get("ts")
                                                                                .and_then(|v| v.as_str())
                                                                                .and_then(|s| s.parse::<i64>().ok())
                                                                                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
                                                                        };

                                                                        // Store and broadcast
                                                                        orderbook_storage.write().insert(internal_symbol.clone(), orderbook_data.clone());
                                                                        ORDERBOOKS.write().push(orderbook_data);
                                                                    }
                                                                }
                                                            },
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("OKX {} chunk {}: Failed to parse JSON: {}",
                                                    asset_type_clone, chunk_idx, e);
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Ping(data))) => {
                                        debug!("OKX {} chunk {}: Received ping, sending pong",
                                            asset_type_clone, chunk_idx);
                                        if ws_stream.send(Message::Pong(data)).await.is_err() {
                                            error!("OKX {} chunk {}: Failed to send pong",
                                                asset_type_clone, chunk_idx);
                                            break;
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) => {
                                        warn!("OKX {} chunk {}: WebSocket closed by server",
                                            asset_type_clone, chunk_idx);
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        error!("OKX {} chunk {}: WebSocket error: {}",
                                            asset_type_clone, chunk_idx, e);
                                        break;
                                    }
                                    None => {
                                        warn!("OKX {} chunk {}: WebSocket stream ended",
                                            asset_type_clone, chunk_idx);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    active_conns.fetch_sub(1, Ordering::SeqCst);
                    warn!("OKX {} chunk {}: Message processing loop ended",
                        asset_type_clone, chunk_idx);
                });

                self.active_connections.fetch_add(1, Ordering::SeqCst);
            }
        }

        info!("OKX: All {} WebSocket handlers spawned", self.active_connections.load(Ordering::SeqCst));
        Ok(())
    }

}