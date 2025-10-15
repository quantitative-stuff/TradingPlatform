/// HFT OrderBook Integration for Feeder
///
/// Integrates the lightweight HFT orderbook processor with exchange feeders

use matching_engine::hft_orderbook::{HFTOrderBookProcessor, FastOrderBookSnapshot};
use market_types::{OrderBookUpdate, Exchange};
use std::sync::{Arc, RwLock};
use once_cell::sync::OnceCell;
use tracing::{info, debug, warn};

/// Global HFT processor instance
static HFT_PROCESSOR: OnceCell<Arc<RwLock<HFTOrderBookProcessor>>> = OnceCell::new();

/// Initialize the global HFT orderbook processor
pub fn init_hft_processor() {
    let processor = HFTOrderBookProcessor::new();

    HFT_PROCESSOR.set(Arc::new(RwLock::new(processor)))
        .unwrap_or_else(|_| warn!("HFT processor already initialized"));

    info!("HFT OrderBook processor initialized");
}

/// Get the global HFT processor
pub fn get_hft_processor() -> Option<Arc<RwLock<HFTOrderBookProcessor>>> {
    HFT_PROCESSOR.get().cloned()
}

/// Start the HFT processor
pub fn start_hft_processor() {
    if let Some(processor) = get_hft_processor() {
        processor.write().unwrap().start_processing();
        info!("HFT OrderBook processor started");
    } else {
        warn!("HFT processor not initialized");
    }
}

/// Register symbols with the HFT processor
pub fn register_symbols(exchange: &str, symbols: &[String]) {
    if let Some(processor) = get_hft_processor() {
        let mut proc = processor.write().unwrap();

        for symbol in symbols {
            // Determine tick size based on symbol
            let tick_size = if symbol.contains("BTC") {
                0.01
            } else if symbol.contains("SHIB") || symbol.contains("PEPE") {
                0.00000001
            } else {
                0.0001
            };

            proc.register_symbol(symbol, tick_size);
        }

        info!("Registered {} symbols for {}", symbols.len(), exchange);
    }
}

/// Process orderbook update from exchange
pub fn process_orderbook_update(update: OrderBookUpdate) {
    if let Some(processor) = get_hft_processor() {
        let proc = processor.read().unwrap();

        // Convert to fast update
        if let Some(fast_update) = proc.convert_update(&update) {
            // Get ring buffer for this exchange
            let ring = proc.get_ring_buffer(update.exchange);

            // Push to ring buffer
            if !ring.push(fast_update) {
                debug!("Ring buffer full for {:?}, dropping update", update.exchange);
            }
        } else {
            debug!("Failed to convert update for symbol: {} (not registered?)", update.symbol);
        }
    } else {
        warn!("HFT processor not initialized");
    }
}

/// Get current orderbook snapshot for a symbol
pub fn get_orderbook_snapshot(symbol: &str) -> Option<FastOrderBookSnapshot> {
    if let Some(processor) = get_hft_processor() {
        let proc = processor.read().unwrap();
        proc.get_orderbook(symbol)
    } else {
        None
    }
}

/// Integration helper for Binance
pub fn integrate_binance_orderbook(
    data: &serde_json::Value,
    symbol: &str,
    stream_name: &str,
) {
    // Parse update_id
    let update_id = data.get("u")
        .or_else(|| data.get("lastUpdateId"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let first_update_id = data.get("U")
        .and_then(|v| v.as_u64())
        .unwrap_or(update_id);

    let prev_update_id = data.get("pu")
        .and_then(|v| v.as_u64());

    // Determine if snapshot
    let is_snapshot = stream_name.contains("@depth") && !stream_name.contains("@depth@");

    // Parse bids
    let mut bids = Vec::new();
    if let Some(bid_array) = data.get("b").or_else(|| data.get("bids")).and_then(|v| v.as_array()) {
        for item in bid_array.iter().take(20) {  // Only take top 20
            if let Some(arr) = item.as_array() {
                if arr.len() >= 2 {
                    if let (Some(price_str), Some(qty_str)) = (arr[0].as_str(), arr[1].as_str()) {
                        if let (Ok(price), Ok(qty)) = (price_str.parse::<f64>(), qty_str.parse::<f64>()) {
                            bids.push(market_types::PriceLevel { price, quantity: qty });
                        }
                    }
                }
            }
        }
    }

    // Parse asks
    let mut asks = Vec::new();
    if let Some(ask_array) = data.get("a").or_else(|| data.get("asks")).and_then(|v| v.as_array()) {
        for item in ask_array.iter().take(20) {  // Only take top 20
            if let Some(arr) = item.as_array() {
                if arr.len() >= 2 {
                    if let (Some(price_str), Some(qty_str)) = (arr[0].as_str(), arr[1].as_str()) {
                        if let (Ok(price), Ok(qty)) = (price_str.parse::<f64>(), qty_str.parse::<f64>()) {
                            asks.push(market_types::PriceLevel { price, quantity: qty });
                        }
                    }
                }
            }
        }
    }

    // Skip empty updates
    if bids.is_empty() && asks.is_empty() {
        debug!("Skipping empty orderbook for {}", symbol);
        return;
    }

    // Create OrderBookUpdate
    let update = OrderBookUpdate {
        exchange: Exchange::Binance,
        symbol: symbol.to_uppercase(),
        timestamp: data.get("E")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_micros()),
        bids,
        asks,
        is_snapshot,
        update_id,
        first_update_id,
        prev_update_id,
    };

    // Log every 100th update for monitoring
    static mut UPDATE_COUNT: u64 = 0;
    unsafe {
        UPDATE_COUNT += 1;
        if UPDATE_COUNT % 100 == 0 {
            info!("HFT Integration: Processing update #{} for {}: {} bids, {} asks, seq {}",
                  UPDATE_COUNT, update.symbol, update.bids.len(), update.asks.len(), update_id);
        }
    }

    debug!("Processing orderbook for {}: {} bids, {} asks, seq {}",
           update.symbol, update.bids.len(), update.asks.len(), update_id);

    // Process with HFT processor
    process_orderbook_update(update);
}