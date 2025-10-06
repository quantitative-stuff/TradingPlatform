use crate::types::{OrderBook, Trade, Side};

pub struct ImbalanceCalculator {}

impl ImbalanceCalculator {
    pub fn new() -> Self {
        Self {}
    }

    /// Order Flow Imbalance (OFI)
    /// Measures the imbalance between buy and sell trade flows
    pub fn order_flow_imbalance(&self, _book: &OrderBook, trades: &[Trade]) -> Option<f64> {
        if trades.is_empty() {
            return None;
        }

        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;

        for trade in trades {
            match trade.side {
                Side::Buy => buy_volume += trade.quantity * trade.price,
                Side::Sell => sell_volume += trade.quantity * trade.price,
            }
        }

        let total_volume = buy_volume + sell_volume;
        if total_volume > 0.0 {
            Some((buy_volume - sell_volume) / total_volume)
        } else {
            None
        }
    }

    /// Order Book Imbalance (OBI)
    /// Measures the imbalance between bid and ask quantities at best levels
    pub fn order_book_imbalance(&self, book: &OrderBook) -> Option<f64> {
        let best_bid_qty = book.bids.first().map(|l| l.quantity)?;
        let best_ask_qty = book.asks.first().map(|l| l.quantity)?;

        let total_qty = best_bid_qty + best_ask_qty;
        if total_qty > 0.0 {
            Some((best_bid_qty - best_ask_qty) / total_qty)
        } else {
            None
        }
    }

    /// Liquidity Imbalance
    /// Measures the imbalance of liquidity at a certain depth
    pub fn liquidity_imbalance(&self, book: &OrderBook, depth_bps: f64) -> Option<f64> {
        let mid = book.mid_price()?;
        let depth_threshold = mid * depth_bps / 10000.0;

        let mut bid_liquidity = 0.0;
        let mut ask_liquidity = 0.0;

        // Calculate bid liquidity
        for level in &book.bids {
            if mid - level.price > depth_threshold {
                break;
            }
            bid_liquidity += level.quantity * level.price;
        }

        // Calculate ask liquidity
        for level in &book.asks {
            if level.price - mid > depth_threshold {
                break;
            }
            ask_liquidity += level.quantity * level.price;
        }

        let total_liquidity = bid_liquidity + ask_liquidity;
        if total_liquidity > 0.0 {
            Some((bid_liquidity - ask_liquidity) / total_liquidity)
        } else {
            None
        }
    }

    /// Queue Imbalance
    /// Measures the imbalance between cumulative bid and ask quantities
    pub fn queue_imbalance(&self, book: &OrderBook) -> Option<f64> {
        let depth = 5; // Use top 5 levels

        let bid_queue: f64 = book.bids
            .iter()
            .take(depth)
            .map(|l| l.quantity)
            .sum();

        let ask_queue: f64 = book.asks
            .iter()
            .take(depth)
            .map(|l| l.quantity)
            .sum();

        let total_queue = bid_queue + ask_queue;
        if total_queue > 0.0 {
            Some((bid_queue - ask_queue) / total_queue)
        } else {
            None
        }
    }

    /// Volume-weighted imbalance
    pub fn volume_weighted_imbalance(&self, book: &OrderBook, levels: usize) -> Option<f64> {
        let mut bid_weighted = 0.0;
        let mut ask_weighted = 0.0;
        let mut total_weight = 0.0;

        // Process bid side
        for (i, level) in book.bids.iter().take(levels).enumerate() {
            let weight = 1.0 / (i as f64 + 1.0); // Decay weight by level
            bid_weighted += level.quantity * weight;
            total_weight += weight;
        }

        // Process ask side
        for (i, level) in book.asks.iter().take(levels).enumerate() {
            let weight = 1.0 / (i as f64 + 1.0);
            ask_weighted += level.quantity * weight;
            total_weight += weight;
        }

        if total_weight > 0.0 && (bid_weighted + ask_weighted) > 0.0 {
            Some((bid_weighted - ask_weighted) / (bid_weighted + ask_weighted))
        } else {
            None
        }
    }

    /// Pressure imbalance (considers both price and quantity)
    pub fn pressure_imbalance(&self, book: &OrderBook) -> Option<f64> {
        let mid = book.mid_price()?;
        let depth = 10; // Use top 10 levels

        let mut bid_pressure = 0.0;
        let mut ask_pressure = 0.0;

        // Calculate bid pressure
        for level in book.bids.iter().take(depth) {
            let distance = (mid - level.price).abs();
            if distance > 0.0 {
                bid_pressure += level.quantity / distance;
            }
        }

        // Calculate ask pressure
        for level in book.asks.iter().take(depth) {
            let distance = (level.price - mid).abs();
            if distance > 0.0 {
                ask_pressure += level.quantity / distance;
            }
        }

        let total_pressure = bid_pressure + ask_pressure;
        if total_pressure > 0.0 {
            Some((bid_pressure - ask_pressure) / total_pressure)
        } else {
            None
        }
    }
}