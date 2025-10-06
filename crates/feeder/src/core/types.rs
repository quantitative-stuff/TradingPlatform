use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use parking_lot::RwLock;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::Notify;
use std::collections::VecDeque;
use tokio::sync::watch;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub exchange: String, //since string takes large memory it should be something else
    pub symbol: String,
    pub asset_type: String, // spot, futures, linear, etc.
    pub price: f64,
    pub quantity: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookData {
    pub exchange: String,
    pub symbol: String,
    pub asset_type: String, // spot, futures, linear, etc.
    pub bids: Vec<(f64, f64)>,  // (price, quantity) - limited to max 50 levels
    pub asks: Vec<(f64, f64)>,  // (price, quantity) - limited to max 50 levels
    pub timestamp: i64,
}

impl OrderBookData {
    pub fn new(exchange: String, symbol: String, asset_type: String, mut bids: Vec<(f64, f64)>, mut asks: Vec<(f64, f64)>, timestamp: i64) -> Self {
        // Limit to 50 levels to prevent memory issues
        bids.truncate(50);
        asks.truncate(50);
        OrderBookData {
            exchange,
            symbol,
            asset_type,
            bids,
            asks,
            timestamp,
        }
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
