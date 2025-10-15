use serde::{Deserialize, Serialize};
use crate::{Exchange, Timestamp, Price, Quantity, Symbol};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Price,
    pub quantity: Quantity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookUpdate {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub timestamp: Timestamp,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub is_snapshot: bool,

    // Sequence tracking for gap detection
    pub update_id: u64,        // Current update sequence number
    pub first_update_id: u64,  // First update ID in this message (for snapshot ranges)
    pub prev_update_id: Option<u64>, // Previous update ID (for continuity check)
}

impl Default for OrderBookUpdate {
    fn default() -> Self {
        Self {
            exchange: Exchange::Binance, // Use a real exchange as default
            symbol: String::new(),  // Symbol is just String type
            timestamp: 0,
            bids: Vec::new(),
            asks: Vec::new(),
            is_snapshot: false,
            update_id: 0,
            first_update_id: 0,
            prev_update_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub timestamp: Timestamp,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,

    // Sequence tracking for maintaining consistency
    pub last_update_id: u64,    // Last applied update sequence number
    pub first_update_id: u64,   // First update ID (for snapshot ranges)
    pub update_count: u64,      // Total updates applied
}

impl OrderBook {
    pub fn new(symbol: Symbol, exchange: Exchange) -> Self {
        Self {
            symbol,
            exchange,
            timestamp: 0,
            bids: Vec::new(),
            asks: Vec::new(),
            last_update_id: 0,
            first_update_id: 0,
            update_count: 0,
        }
    }

    pub fn best_bid(&self) -> Option<&PriceLevel> {
        self.bids.first()
    }

    pub fn best_ask(&self) -> Option<&PriceLevel> {
        self.asks.first()
    }

    pub fn mid_price(&self) -> Option<Price> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / 2.0),
            _ => None,
        }
    }

    pub fn spread(&self) -> Option<Price> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    /// Calculate Weighted Average Price (WAP) at specified depth
    pub fn wap(&self, depth_bps: f64) -> Option<f64> {
        let mid = self.mid_price()?;
        let depth_threshold = mid * depth_bps / 10000.0;

        let mut bid_value = 0.0;
        let mut bid_volume = 0.0;
        let mut ask_value = 0.0;
        let mut ask_volume = 0.0;

        // Accumulate bid side
        for level in &self.bids {
            if mid - level.price > depth_threshold {
                break;
            }
            bid_value += level.price * level.quantity;
            bid_volume += level.quantity;
        }

        // Accumulate ask side
        for level in &self.asks {
            if level.price - mid > depth_threshold {
                break;
            }
            ask_value += level.price * level.quantity;
            ask_volume += level.quantity;
        }

        let total_volume = bid_volume + ask_volume;
        if total_volume > 0.0 {
            Some((bid_value + ask_value) / total_volume)
        } else {
            None
        }
    }
}
