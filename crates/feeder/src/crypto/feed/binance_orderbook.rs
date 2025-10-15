/// Binance orderbook integration module
///
/// Extends Binance feeder with OrderBookBuilder integration

use serde_json::Value;
use market_types::{OrderBookUpdate, Exchange, PriceLevel};
use crate::core::{get_orderbook_manager, SymbolMapper};
use std::sync::Arc;
use tracing::{debug, warn};

/// Process Binance depth update and send to OrderBookBuilder
pub fn process_binance_orderbook(
    data: &Value,
    symbol: &str,
    stream_name: &str,
    symbol_mapper: Arc<SymbolMapper>,
) {
    // Map to common symbol
    let common_symbol = match symbol_mapper.map("Binance", symbol) {
        Some(s) => s,
        None => {
            // Symbol not mapped, skip silently
            return;
        }
    };

    // Get OrderBook manager
    let manager = match get_orderbook_manager() {
        Some(m) => m,
        None => {
            debug!("OrderBook manager not initialized");
            return;
        }
    };

    // Determine if this is a snapshot or incremental update
    let is_snapshot = stream_name.contains("@depth") && !stream_name.contains("@depth@");

    // Parse sequence numbers
    let update_id = data.get("u")
        .or_else(|| data.get("lastUpdateId"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let first_update_id = data.get("U")
        .and_then(|v| v.as_u64())
        .unwrap_or(update_id);

    let prev_update_id = data.get("pu")
        .and_then(|v| v.as_u64());

    // Parse bids
    let bids: Vec<PriceLevel> = data.get("b")
        .or_else(|| data.get("bids"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if let Some(arr) = item.as_array() {
                        if arr.len() >= 2 {
                            let price = arr[0].as_str()?.parse::<f64>().ok()?;
                            let qty = arr[1].as_str()?.parse::<f64>().ok()?;
                            return Some(PriceLevel { price, quantity: qty });
                        }
                    }
                    None
                })
                .collect::<Vec<PriceLevel>>()
        })
        .unwrap_or_default();

    // Parse asks
    let asks: Vec<PriceLevel> = data.get("a")
        .or_else(|| data.get("asks"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if let Some(arr) = item.as_array() {
                        if arr.len() >= 2 {
                            let price = arr[0].as_str()?.parse::<f64>().ok()?;
                            let qty = arr[1].as_str()?.parse::<f64>().ok()?;
                            return Some(PriceLevel { price, quantity: qty });
                        }
                    }
                    None
                })
                .collect::<Vec<PriceLevel>>()
        })
        .unwrap_or_default();

    // Skip empty updates unless it's a snapshot
    if !is_snapshot && bids.is_empty() && asks.is_empty() {
        return;
    }

    // Create OrderBookUpdate
    let update = OrderBookUpdate {
        exchange: Exchange::Binance,
        symbol: common_symbol.clone(),
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

    // Send to OrderBookBuilder asynchronously
    let manager_clone = manager.clone();
    tokio::spawn(async move {
        if let Err(e) = manager_clone.try_send_update(update).await {
            debug!("Failed to send orderbook update: {}", e);
        }
    });
}

/// Check if message contains orderbook data
pub fn is_orderbook_message(stream_name: &str, data: &Value) -> bool {
    // Check stream name
    if stream_name.contains("@depth") {
        return true;
    }

    // Check data structure
    if (data.get("b").is_some() || data.get("bids").is_some()) &&
       (data.get("a").is_some() || data.get("asks").is_some()) {
        return true;
    }

    false
}