pub mod wap;
pub mod imbalance;
pub mod flow;

use crate::types::{MarketFeatures, OrderBook, Trade, Timestamp, Exchange};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;

const MAX_HISTORY_SIZE: usize = 10000;

pub struct FeatureEngine {
    // Historical data storage
    orderbook_history: Arc<DashMap<String, Arc<RwLock<VecDeque<OrderBook>>>>>,
    trade_history: Arc<DashMap<String, Arc<RwLock<VecDeque<Trade>>>>>,

    // Feature calculators
    wap_calculator: wap::WAPCalculator,
    imbalance_calculator: imbalance::ImbalanceCalculator,
    flow_calculator: flow::FlowCalculator,
}

impl FeatureEngine {
    pub fn new() -> Self {
        Self {
            orderbook_history: Arc::new(DashMap::new()),
            trade_history: Arc::new(DashMap::new()),
            wap_calculator: wap::WAPCalculator::new(),
            imbalance_calculator: imbalance::ImbalanceCalculator::new(),
            flow_calculator: flow::FlowCalculator::new(),
        }
    }

    pub fn calculate_features(
        &self,
        symbol: &str,
        exchange: Exchange,
        current_book: &OrderBook,
        recent_trades: &[Trade],
        timestamp: Timestamp,
    ) -> MarketFeatures {
        // Get historical data
        let book_history = self.get_orderbook_history(symbol);
        let trade_history = self.get_trade_history(symbol);

        // Calculate WAP at different lags
        let wap_0ms = self.wap_calculator.calculate(current_book, 0);
        let wap_100ms = self.wap_calculator.calculate_lagged(
            &book_history,
            timestamp - 100_000, // 100ms in microseconds
            10.0  // 10 bps depth
        );
        let wap_200ms = self.wap_calculator.calculate_lagged(
            &book_history,
            timestamp - 200_000,
            10.0
        );
        let wap_300ms = self.wap_calculator.calculate_lagged(
            &book_history,
            timestamp - 300_000,
            10.0
        );

        // Calculate imbalance features
        let order_flow_imbalance = self.imbalance_calculator
            .order_flow_imbalance(current_book, recent_trades);
        let order_book_imbalance = self.imbalance_calculator
            .order_book_imbalance(current_book);
        let liquidity_imbalance = self.imbalance_calculator
            .liquidity_imbalance(current_book, 20.0); // 20 bps depth
        let queue_imbalance = self.imbalance_calculator
            .queue_imbalance(current_book);

        // Calculate flow features
        let tick_flow_imbalance = self.flow_calculator
            .tick_flow_imbalance(&trade_history, timestamp, 15_000_000); // 15 seconds
        let trade_flow_imbalance = self.flow_calculator
            .trade_flow_imbalance(&trade_history, timestamp, 15_000_000);

        // Calculate spread features
        let spread = current_book.spread();
        let spread_bps = if let (Some(sp), Some(mid)) = (spread, current_book.mid_price()) {
            Some(sp / mid * 10000.0)
        } else {
            None
        };

        // Calculate volume features at different depths
        let (bid_volume_10bps, ask_volume_10bps) = self.calculate_volume_at_depth(
            current_book,
            10.0
        );
        let (bid_volume_20bps, ask_volume_20bps) = self.calculate_volume_at_depth(
            current_book,
            20.0
        );

        MarketFeatures {
            timestamp,
            symbol: symbol.to_string(),
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
            bid_volume_10bps,
            ask_volume_10bps,
            bid_volume_20bps,
            ask_volume_20bps,
        }
    }

    pub fn update_orderbook(&self, symbol: &str, book: OrderBook) {
        let history = self.orderbook_history
            .entry(symbol.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(VecDeque::new())))
            .clone();

        let mut history_guard = history.write();
        history_guard.push_back(book);

        // Maintain max history size
        while history_guard.len() > MAX_HISTORY_SIZE {
            history_guard.pop_front();
        }
    }

    pub fn update_trade(&self, symbol: &str, trade: Trade) {
        let history = self.trade_history
            .entry(symbol.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(VecDeque::new())))
            .clone();

        let mut history_guard = history.write();
        history_guard.push_back(trade);

        // Maintain max history size
        while history_guard.len() > MAX_HISTORY_SIZE {
            history_guard.pop_front();
        }
    }

    fn get_orderbook_history(&self, symbol: &str) -> Vec<OrderBook> {
        self.orderbook_history
            .get(symbol)
            .map(|history| history.read().iter().cloned().collect())
            .unwrap_or_default()
    }

    fn get_trade_history(&self, symbol: &str) -> Vec<Trade> {
        self.trade_history
            .get(symbol)
            .map(|history| history.read().iter().cloned().collect())
            .unwrap_or_default()
    }

    fn calculate_volume_at_depth(
        &self,
        book: &OrderBook,
        depth_bps: f64
    ) -> (Option<f64>, Option<f64>) {
        let mid = match book.mid_price() {
            Some(m) => m,
            None => return (None, None),
        };

        let depth_threshold = mid * depth_bps / 10000.0;

        let mut bid_volume = 0.0;
        for level in &book.bids {
            if mid - level.price > depth_threshold {
                break;
            }
            bid_volume += level.quantity;
        }

        let mut ask_volume = 0.0;
        for level in &book.asks {
            if level.price - mid > depth_threshold {
                break;
            }
            ask_volume += level.quantity;
        }

        (Some(bid_volume), Some(ask_volume))
    }
}