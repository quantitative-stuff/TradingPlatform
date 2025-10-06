use market_types::{
    OrderBook, Trade, OrderEvent, MarketFeatures, Exchange, Side
};
use crate::history::{OrderBookHistory, TradeHistory};
use parking_lot::RwLock;
use std::sync::Arc;

/// Feature calculator for market microstructure features
pub struct FeatureCalculator {
    book_history: Arc<RwLock<OrderBookHistory>>,
    trade_history: Arc<RwLock<TradeHistory>>,
}

impl FeatureCalculator {
    pub fn new(lookback_ms: i64) -> Self {
        Self {
            book_history: Arc::new(RwLock::new(OrderBookHistory::new(lookback_ms))),
            trade_history: Arc::new(RwLock::new(TradeHistory::new(lookback_ms))),
        }
    }

    /// Add new order book snapshot to history
    pub fn update_order_book(&self, book: OrderBook) {
        self.book_history.write().add_snapshot(book);
    }

    /// Add new trade to history
    pub fn update_trade(&self, trade: Trade) {
        self.trade_history.write().add_trade(trade);
    }

    /// Calculate all market features for current state
    pub fn calculate_features(&self, symbol: String, exchange: Exchange) -> Option<MarketFeatures> {
        let book_history = self.book_history.read();
        let trade_history = self.trade_history.read();

        let current_book = book_history.latest()?;
        let timestamp = current_book.timestamp;

        // Calculate WAP at different time horizons
        let wap_0ms = current_book.wap(10.0); // 10 bps depth
        let wap_100ms = book_history.get_book_at(100).and_then(|b| b.wap(10.0));
        let wap_200ms = book_history.get_book_at(200).and_then(|b| b.wap(10.0));
        let wap_300ms = book_history.get_book_at(300).and_then(|b| b.wap(10.0));

        // Calculate order book imbalances
        let order_book_imbalance = Self::calc_order_book_imbalance(current_book);
        let liquidity_imbalance = Self::calc_liquidity_imbalance(current_book, 10.0);
        let queue_imbalance = Self::calc_queue_imbalance(current_book);

        // Calculate trade flow features
        let tick_flow_imbalance = Self::calc_tick_flow_imbalance(&trade_history, 1000);
        let trade_flow_imbalance = Self::calc_trade_flow_imbalance(&trade_history, 1000);

        // Calculate spread features
        let spread = current_book.spread();
        let spread_bps = spread.and_then(|s| {
            current_book.mid_price().map(|mid| s / mid * 10000.0)
        });

        // Calculate volume features
        let (bid_volume_10bps, ask_volume_10bps) = Self::calc_volume_at_depth(current_book, 10.0);
        let (bid_volume_20bps, ask_volume_20bps) = Self::calc_volume_at_depth(current_book, 20.0);

        // OFI calculation (requires order events - placeholder for now)
        let order_flow_imbalance = None; // TODO: Implement with OrderEventHistory

        Some(MarketFeatures {
            timestamp,
            symbol,
            exchange,
            wap_0ms,
            wap_100ms,
            wap_200ms,
            wap_300ms,
            order_flow_imbalance,
            order_book_imbalance,
            liquidity_imbalance,
            queue_imbalance,
            tick_flow_imbalance,
            trade_flow_imbalance,
            spread,
            spread_bps,
            bid_volume_10bps: Some(bid_volume_10bps),
            ask_volume_10bps: Some(ask_volume_10bps),
            bid_volume_20bps: Some(bid_volume_20bps),
            ask_volume_20bps: Some(ask_volume_20bps),
        })
    }

    /// Calculate order book imbalance at best bid/ask
    fn calc_order_book_imbalance(book: &OrderBook) -> Option<f64> {
        let best_bid_qty = book.bids.first()?.quantity;
        let best_ask_qty = book.asks.first()?.quantity;

        Some((best_bid_qty - best_ask_qty) / (best_bid_qty + best_ask_qty))
    }

    /// Calculate liquidity imbalance within depth threshold
    fn calc_liquidity_imbalance(book: &OrderBook, depth_bps: f64) -> Option<f64> {
        let mid = book.mid_price()?;
        let threshold = mid * depth_bps / 10000.0;

        let mut bid_liquidity = 0.0;
        let mut ask_liquidity = 0.0;

        // Sum bid side
        for level in &book.bids {
            if mid - level.price > threshold {
                break;
            }
            bid_liquidity += level.quantity;
        }

        // Sum ask side
        for level in &book.asks {
            if level.price - mid > threshold {
                break;
            }
            ask_liquidity += level.quantity;
        }

        let total = bid_liquidity + ask_liquidity;
        if total > 0.0 {
            Some((bid_liquidity - ask_liquidity) / total)
        } else {
            None
        }
    }

    /// Calculate queue position imbalance (bid vs ask depth)
    fn calc_queue_imbalance(book: &OrderBook) -> Option<f64> {
        let bid_depth = book.bids.len() as f64;
        let ask_depth = book.asks.len() as f64;

        if bid_depth + ask_depth > 0.0 {
            Some((bid_depth - ask_depth) / (bid_depth + ask_depth))
        } else {
            None
        }
    }

    /// Calculate tick flow imbalance (directional trade count)
    fn calc_tick_flow_imbalance(trade_history: &TradeHistory, lookback_ms: i64) -> Option<f64> {
        let mut buy_ticks = 0.0;
        let mut sell_ticks = 0.0;

        for trade in trade_history.trades_since(lookback_ms) {
            match trade.side {
                Side::Buy => buy_ticks += 1.0,
                Side::Sell => sell_ticks += 1.0,
            }
        }

        let total = buy_ticks + sell_ticks;
        if total > 0.0 {
            Some((buy_ticks - sell_ticks) / total)
        } else {
            None
        }
    }

    /// Calculate trade flow imbalance (volume-weighted)
    fn calc_trade_flow_imbalance(trade_history: &TradeHistory, lookback_ms: i64) -> Option<f64> {
        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;

        for trade in trade_history.trades_since(lookback_ms) {
            match trade.side {
                Side::Buy => buy_volume += trade.quantity,
                Side::Sell => sell_volume += trade.quantity,
            }
        }

        let total = buy_volume + sell_volume;
        if total > 0.0 {
            Some((buy_volume - sell_volume) / total)
        } else {
            None
        }
    }

    /// Calculate total volume within depth threshold
    fn calc_volume_at_depth(book: &OrderBook, depth_bps: f64) -> (f64, f64) {
        let mid = match book.mid_price() {
            Some(m) => m,
            None => return (0.0, 0.0),
        };

        let threshold = mid * depth_bps / 10000.0;

        let mut bid_volume = 0.0;
        for level in &book.bids {
            if mid - level.price > threshold {
                break;
            }
            bid_volume += level.quantity;
        }

        let mut ask_volume = 0.0;
        for level in &book.asks {
            if level.price - mid > threshold {
                break;
            }
            ask_volume += level.quantity;
        }

        (bid_volume, ask_volume)
    }

    /// Calculate Order Flow Imbalance (OFI)
    /// OFI = Σ(buy_orders - sell_orders) over time window
    pub fn calc_ofi(events: &[OrderEvent], lookback_ms: i64) -> Option<f64> {
        if events.is_empty() {
            return None;
        }

        let latest_time = match events.last()? {
            OrderEvent::Add { timestamp, .. } => *timestamp,
            OrderEvent::Modify { timestamp, .. } => *timestamp,
            OrderEvent::Cancel { timestamp, .. } => *timestamp,
            OrderEvent::Execute { timestamp, .. } => *timestamp,
        };

        let cutoff = latest_time - lookback_ms * 1000;

        let mut buy_flow = 0.0;
        let mut sell_flow = 0.0;

        for event in events {
            let (timestamp, side, qty) = match event {
                OrderEvent::Add { timestamp, side, quantity, .. } => {
                    (*timestamp, *side, *quantity)
                }
                OrderEvent::Modify { .. } => continue, // Skip modifications
                OrderEvent::Cancel { .. } => continue, // Skip cancellations
                OrderEvent::Execute { .. } => {
                    // For executions, we don't have side info in current implementation
                    // This would need enhancement
                    continue;
                }
            };

            if timestamp < cutoff {
                continue;
            }

            match side {
                Side::Buy => buy_flow += qty,
                Side::Sell => sell_flow += qty,
            }
        }

        let total = buy_flow + sell_flow;
        if total > 0.0 {
            Some((buy_flow - sell_flow) / total)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_types::PriceLevel;

    #[test]
    fn test_order_book_imbalance() {
        let mut book = OrderBook::new("BTCUSDT".to_string(), Exchange::Binance);
        book.bids = vec![
            PriceLevel { price: 100.0, quantity: 10.0 },
        ];
        book.asks = vec![
            PriceLevel { price: 101.0, quantity: 5.0 },
        ];

        let imbalance = FeatureCalculator::calc_order_book_imbalance(&book).unwrap();
        assert!((imbalance - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_liquidity_imbalance() {
        let mut book = OrderBook::new("BTCUSDT".to_string(), Exchange::Binance);
        book.bids = vec![
            PriceLevel { price: 100.0, quantity: 10.0 },
            PriceLevel { price: 99.0, quantity: 5.0 },
        ];
        book.asks = vec![
            PriceLevel { price: 101.0, quantity: 3.0 },
        ];

        let imbalance = FeatureCalculator::calc_liquidity_imbalance(&book, 200.0).unwrap();
        assert!(imbalance > 0.0); // More bid liquidity
    }
}
