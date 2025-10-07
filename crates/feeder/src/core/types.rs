use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use parking_lot::RwLock;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::Notify;
use std::collections::VecDeque;
use tokio::sync::watch;

// HFT-optimized normalized data structures with scaled integers
// Following the recommendations from timestamp_price_quantity.md

/// Normalized trade data with nanosecond timestamps and scaled integer prices/quantities
/// This struct uses fixed-size integers for cache efficiency and exact arithmetic
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct NormalizedTrade {
    pub ts: u64,           // Unix nanoseconds since epoch
    pub price: i64,        // Scaled by 1e8 (100,000,000) for 8 decimal precision
    pub quantity: i64,     // Scaled by 1e8 (100,000,000) for 8 decimal precision
    pub side: u8,          // 0 = Sell, 1 = Buy
    pub exchange_id: u8,   // Exchange identifier (to be mapped)
    pub symbol_id: u16,    // Symbol identifier (to be mapped)
}

/// Normalized order book level with scaled integers
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct NormalizedOrderBookLevel {
    pub price: i64,        // Scaled by 1e8
    pub quantity: i64,     // Scaled by 1e8
}

/// Normalized order book data
#[derive(Debug, Clone)]
pub struct NormalizedOrderBook {
    pub ts: u64,           // Unix nanoseconds since epoch
    pub exchange_id: u8,
    pub symbol_id: u16,
    pub bids: Vec<NormalizedOrderBookLevel>,
    pub asks: Vec<NormalizedOrderBookLevel>,
}

// Scaling constant for price/quantity normalization
pub const PRICE_SCALE: i64 = 100_000_000;  // 1e8 - provides 8 decimal places
pub const QTY_SCALE: i64 = 100_000_000;    // 1e8 - provides 8 decimal places

/// Parse price/quantity string directly to scaled i64 (AVOIDS f64!)
/// Examples:
/// - parse_to_scaled("95123.45", 8) → 9512345000000
/// - parse_to_scaled("0.00000123", 8) → 123
/// - parse_to_scaled("100", 8) → 10000000000
pub fn parse_to_scaled(value: &str, precision: u8) -> Option<i64> {
    let parts: Vec<&str> = value.split('.').collect();

    match parts.len() {
        // No decimal point: "100" → "100.00000000"
        1 => {
            let integer_part: i64 = parts[0].parse().ok()?;
            let scale = 10_i64.pow(precision as u32);
            Some(integer_part.checked_mul(scale)?)
        },
        // Has decimal: "95123.45"
        2 => {
            let integer_part: i64 = if parts[0].is_empty() { 0 } else { parts[0].parse().ok()? };
            let decimal_str = parts[1];

            // Pad or truncate decimal part to match precision
            let mut decimal_value = String::from(decimal_str);

            if decimal_value.len() < precision as usize {
                // Pad with zeros: "45" → "45000000" for precision=8
                decimal_value.push_str(&"0".repeat(precision as usize - decimal_value.len()));
            } else if decimal_value.len() > precision as usize {
                // Truncate: "123456789" → "12345678" for precision=8
                decimal_value.truncate(precision as usize);
            }

            let decimal_part: i64 = decimal_value.parse().ok()?;
            let scale = 10_i64.pow(precision as u32);

            Some(integer_part.checked_mul(scale)?.checked_add(decimal_part)?)
        },
        _ => None, // Invalid format
    }
}

/// Parse and scale, with fallback to 0
pub fn parse_to_scaled_or_default(value: &str, precision: u8) -> i64 {
    parse_to_scaled(value, precision).unwrap_or(0)
}

#[cfg(test)]
mod scaling_tests {
    use super::*;

    #[test]
    fn test_parse_to_scaled() {
        // Integer values
        assert_eq!(parse_to_scaled("100", 8), Some(10000000000));
        assert_eq!(parse_to_scaled("1", 8), Some(100000000));

        // Decimal values
        assert_eq!(parse_to_scaled("95123.45", 8), Some(9512345000000));
        assert_eq!(parse_to_scaled("0.00000123", 8), Some(123));

        // Edge cases
        assert_eq!(parse_to_scaled("0", 8), Some(0));
        assert_eq!(parse_to_scaled("0.0", 8), Some(0));

        // Truncation
        assert_eq!(parse_to_scaled("1.123456789", 8), Some(112345678));

        // Padding
        assert_eq!(parse_to_scaled("1.1", 8), Some(110000000));
    }
}

impl NormalizedTrade {
    /// Create a normalized trade from raw TradeData
    /// Requires exchange_id and symbol_id to be provided from a mapping table
    pub fn from_trade_data(trade: &TradeData, exchange_id: u8, symbol_id: u16) -> Self {
        // Convert timestamp to nanoseconds
        let ts = trade.timestamp_unit.to_nanoseconds(trade.timestamp);

        // Convert from trade's precision to normalized precision (8 decimals = 1e8)
        const TARGET_PRECISION: u8 = 8;
        let price = if trade.price_precision < TARGET_PRECISION {
            trade.price * 10_i64.pow((TARGET_PRECISION - trade.price_precision) as u32)
        } else if trade.price_precision > TARGET_PRECISION {
            trade.price / 10_i64.pow((trade.price_precision - TARGET_PRECISION) as u32)
        } else {
            trade.price
        };

        let quantity = if trade.quantity_precision < TARGET_PRECISION {
            trade.quantity * 10_i64.pow((TARGET_PRECISION - trade.quantity_precision) as u32)
        } else if trade.quantity_precision > TARGET_PRECISION {
            trade.quantity / 10_i64.pow((trade.quantity_precision - TARGET_PRECISION) as u32)
        } else {
            trade.quantity
        };

        // Determine side (buy = 1, sell = 0, default to 0 for unknown)
        // This is a simple heuristic; real implementation should use actual side info
        let side = if trade.quantity > 0 { 1 } else { 0 };

        NormalizedTrade {
            ts,
            price,
            quantity: quantity.abs(), // Store absolute value
            side,
            exchange_id,
            symbol_id,
        }
    }

    /// Convert normalized trade back to floating point price
    pub fn get_price(&self) -> f64 {
        self.price as f64 / PRICE_SCALE as f64
    }

    /// Convert normalized trade back to floating point quantity
    pub fn get_quantity(&self) -> f64 {
        self.quantity as f64 / QTY_SCALE as f64
    }

    /// Check if this is a buy trade
    pub fn is_buy(&self) -> bool {
        self.side == 1
    }
}

impl NormalizedOrderBookLevel {
    /// Create a normalized level from already-scaled i64 values
    pub fn new(price: i64, quantity: i64) -> Self {
        NormalizedOrderBookLevel {
            price,
            quantity,
        }
    }

    /// Create a normalized level from floating point price and quantity
    pub fn from_f64(price: f64, quantity: f64) -> Self {
        NormalizedOrderBookLevel {
            price: (price * PRICE_SCALE as f64).round() as i64,
            quantity: (quantity * QTY_SCALE as f64).round() as i64,
        }
    }

    /// Convert back to floating point price
    pub fn get_price(&self) -> f64 {
        self.price as f64 / PRICE_SCALE as f64
    }

    /// Convert back to floating point quantity
    pub fn get_quantity(&self) -> f64 {
        self.quantity as f64 / QTY_SCALE as f64
    }
}

impl NormalizedOrderBook {
    /// Create a normalized order book from raw OrderBookData
    pub fn from_orderbook_data(orderbook: &OrderBookData, exchange_id: u8, symbol_id: u16) -> Self {
        let ts = orderbook.timestamp_unit.to_nanoseconds(orderbook.timestamp);

        let bids = orderbook.bids.iter()
            .map(|(price, qty)| NormalizedOrderBookLevel::new(*price, *qty))
            .collect();

        let asks = orderbook.asks.iter()
            .map(|(price, qty)| NormalizedOrderBookLevel::new(*price, *qty))
            .collect();

        NormalizedOrderBook {
            ts,
            exchange_id,
            symbol_id,
            bids,
            asks,
        }
    }
}

// Maximum number of items to keep in memory
const MAX_TRADES: usize = 10000;
const MAX_ORDERBOOKS: usize = 5000;

// Circular buffer implementation using VecDeque
pub struct CircularBuffer<T> {
    buffer: VecDeque<T>,
    capacity: usize,
}

impl<T> CircularBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        CircularBuffer {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        }
    }
    
    pub fn push(&mut self, item: T) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front(); // Remove oldest item
        }
        self.buffer.push_back(item);
    }
    
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.buffer.iter()
    }
    
    pub fn len(&self) -> usize {
        self.buffer.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
    
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

// Global state using circular buffers
pub static TRADES: Lazy<Arc<RwLock<CircularBuffer<TradeData>>>> = 
    Lazy::new(|| Arc::new(RwLock::new(CircularBuffer::new(MAX_TRADES))));

pub static ORDERBOOKS: Lazy<Arc<RwLock<CircularBuffer<OrderBookData>>>> = 
    Lazy::new(|| Arc::new(RwLock::new(CircularBuffer::new(MAX_ORDERBOOKS))));

// This notify channel is used to signal that new market data has been added
pub static COMPARE_NOTIFY: Lazy<Notify> = Lazy::new(|| Notify::new());

// Global shutdown signal - when set to true, all tasks should exit
pub static SHUTDOWN_SIGNAL: Lazy<Arc<RwLock<watch::Sender<bool>>>> = Lazy::new(|| {
    let (tx, _rx) = watch::channel(false);
    Arc::new(RwLock::new(tx))
});

pub fn get_shutdown_receiver() -> watch::Receiver<bool> {
    SHUTDOWN_SIGNAL.read().subscribe()
}

pub fn trigger_shutdown() {
    let _ = SHUTDOWN_SIGNAL.read().send(true);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketData {
    pub exchange: String,
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub data_type: MarketDataType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketDataType {
    Trade(TradeData),
    OrderBook(OrderBookData),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TradeData {
    #[serde(default)]
    pub exchange: String, //since string takes large memory it should be something else
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub asset_type: String, // spot, futures, linear, etc.
    #[serde(default)]
    pub price: i64,        // Scaled integer (actual price * 10^price_precision)
    #[serde(default)]
    pub quantity: i64,     // Scaled integer (actual quantity * 10^qty_precision)
    #[serde(default)]
    pub price_precision: u8,    // Number of decimals (e.g., 8 means scale = 1e8)
    #[serde(default)]
    pub quantity_precision: u8, // Number of decimals
    #[serde(default)]
    pub timestamp: u64, // Unix timestamp (unit specified by timestamp_unit)
    #[serde(default)]
    pub timestamp_unit: crate::load_config::TimestampUnit,
}

impl TradeData {
    /// Get actual price as f64 (only for display/logging)
    pub fn get_price_f64(&self) -> f64 {
        self.price as f64 / 10_f64.powi(self.price_precision as i32)
    }

    /// Get actual quantity as f64 (only for display/logging)
    pub fn get_quantity_f64(&self) -> f64 {
        self.quantity as f64 / 10_f64.powi(self.quantity_precision as i32)
    }

    /// Create from f64 values (for backward compatibility during migration)
    pub fn from_f64(
        exchange: String,
        symbol: String,
        asset_type: String,
        price_f64: f64,
        quantity_f64: f64,
        price_precision: u8,
        quantity_precision: u8,
        timestamp: u64,
        timestamp_unit: crate::load_config::TimestampUnit,
    ) -> Self {
        let price_scale = 10_i64.pow(price_precision as u32);
        let qty_scale = 10_i64.pow(quantity_precision as u32);

        TradeData {
            exchange,
            symbol,
            asset_type,
            price: (price_f64 * price_scale as f64).round() as i64,
            quantity: (quantity_f64 * qty_scale as f64).round() as i64,
            price_precision,
            quantity_precision,
            timestamp,
            timestamp_unit,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrderBookData {
    #[serde(default)]
    pub exchange: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub asset_type: String, // spot, futures, linear, etc.
    #[serde(default)]
    pub bids: Vec<(i64, i64)>,  // (scaled_price, scaled_quantity) - limited to max 50 levels
    #[serde(default)]
    pub asks: Vec<(i64, i64)>,  // (scaled_price, scaled_quantity) - limited to max 50 levels
    #[serde(default)]
    pub price_precision: u8,    // Number of decimals
    #[serde(default)]
    pub quantity_precision: u8, // Number of decimals
    #[serde(default)]
    pub timestamp: u64, // Unix timestamp (unit specified by timestamp_unit)
    #[serde(default)]
    pub timestamp_unit: crate::load_config::TimestampUnit,
}

impl OrderBookData {
    /// Create from scaled integer values
    pub fn new(
        exchange: String,
        symbol: String,
        asset_type: String,
        mut bids: Vec<(i64, i64)>,
        mut asks: Vec<(i64, i64)>,
        price_precision: u8,
        quantity_precision: u8,
        timestamp: u64,
        timestamp_unit: crate::load_config::TimestampUnit,
    ) -> Self {
        // Limit to 50 levels to prevent memory issues
        bids.truncate(50);
        asks.truncate(50);
        OrderBookData {
            exchange,
            symbol,
            asset_type,
            bids,
            asks,
            price_precision,
            quantity_precision,
            timestamp,
            timestamp_unit,
        }
    }

    /// Create from f64 values (for backward compatibility during migration)
    pub fn from_f64(
        exchange: String,
        symbol: String,
        asset_type: String,
        bids_f64: Vec<(f64, f64)>,
        asks_f64: Vec<(f64, f64)>,
        price_precision: u8,
        quantity_precision: u8,
        timestamp: u64,
        timestamp_unit: crate::load_config::TimestampUnit,
    ) -> Self {
        let price_scale = 10_i64.pow(price_precision as u32);
        let qty_scale = 10_i64.pow(quantity_precision as u32);

        let bids = bids_f64.iter()
            .map(|(p, q)| (
                (p * price_scale as f64).round() as i64,
                (q * qty_scale as f64).round() as i64,
            ))
            .collect();

        let asks = asks_f64.iter()
            .map(|(p, q)| (
                (p * price_scale as f64).round() as i64,
                (q * qty_scale as f64).round() as i64,
            ))
            .collect();

        Self::new(exchange, symbol, asset_type, bids, asks, price_precision, quantity_precision, timestamp, timestamp_unit)
    }

    /// Get best bid as f64 (for display only)
    pub fn get_best_bid_f64(&self) -> Option<(f64, f64)> {
        self.bids.first().map(|(p, q)| {
            let price_scale = 10_f64.powi(self.price_precision as i32);
            let qty_scale = 10_f64.powi(self.quantity_precision as i32);
            (*p as f64 / price_scale, *q as f64 / qty_scale)
        })
    }

    /// Get best ask as f64 (for display only)
    pub fn get_best_ask_f64(&self) -> Option<(f64, f64)> {
        self.asks.first().map(|(p, q)| {
            let price_scale = 10_f64.powi(self.price_precision as i32);
            let qty_scale = 10_f64.powi(self.quantity_precision as i32);
            (*p as f64 / price_scale, *q as f64 / qty_scale)
        })
    }

    /// Unscale an i64 price value to f64 for display
    pub fn unscale_price(&self, price: i64) -> f64 {
        price as f64 / 10_f64.powi(self.price_precision as i32)
    }

    /// Unscale an i64 quantity value to f64 for display
    pub fn unscale_quantity(&self, qty: i64) -> f64 {
        qty as f64 / 10_f64.powi(self.quantity_precision as i32)
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMapper {
    // A mapping for each exchange.
    // For example, the mapping for "Binance" could be:
    // { "BTCUSDT": "BTC-USD", "ETHUSDT": "ETH-USD", ... }
    pub exchanges: HashMap<String, HashMap<String, String>>,
}

impl SymbolMapper {
    pub fn new() -> Self {
        Self {
            exchanges: HashMap::new(),
        }
    }

    /// Adds a mapping for a specific exchange from its original symbol to your common symbol.
    pub fn add_mapping(&mut self, exchange: &str, original: &str, common: &str) {
        self.exchanges
            .entry(exchange.to_string())
            .or_insert_with(HashMap::new)
            .insert(original.to_string(), common.to_string());

    }

    /// Tries to map an exchange-specific symbol to a common symbol.
    pub fn map(&self, exchange: &str, original: &str) -> Option<String> {
        self.exchanges
            .get(exchange)
            .and_then(|map| map.get(original))
            .cloned()
    }
}
