use crate::types::{Trade, Side, Timestamp};

pub struct FlowCalculator {}

impl FlowCalculator {
    pub fn new() -> Self {
        Self {}
    }

    /// Tick Flow Imbalance
    /// Measures the imbalance between upticks and downticks
    pub fn tick_flow_imbalance(
        &self,
        trades: &[Trade],
        current_timestamp: Timestamp,
        window_us: i64
    ) -> Option<f64> {
        let cutoff = current_timestamp - window_us;
        let recent_trades: Vec<&Trade> = trades
            .iter()
            .filter(|t| t.timestamp >= cutoff)
            .collect();

        if recent_trades.len() < 2 {
            return None;
        }

        let mut upticks = 0;
        let mut downticks = 0;
        let mut unchanged = 0;

        for i in 1..recent_trades.len() {
            let price_change = recent_trades[i].price - recent_trades[i - 1].price;
            if price_change > 0.0 {
                upticks += 1;
            } else if price_change < 0.0 {
                downticks += 1;
            } else {
                unchanged += 1;
            }
        }

        let total_ticks = upticks + downticks;
        if total_ticks > 0 {
            Some((upticks as f64 - downticks as f64) / total_ticks as f64)
        } else {
            None
        }
    }

    /// Trade Flow Imbalance
    /// Measures the imbalance in trade volumes
    pub fn trade_flow_imbalance(
        &self,
        trades: &[Trade],
        current_timestamp: Timestamp,
        window_us: i64
    ) -> Option<f64> {
        let cutoff = current_timestamp - window_us;

        let mut buy_flow = 0.0;
        let mut sell_flow = 0.0;

        for trade in trades {
            if trade.timestamp >= cutoff {
                let flow = trade.price * trade.quantity;
                match trade.side {
                    Side::Buy => buy_flow += flow,
                    Side::Sell => sell_flow += flow,
                }
            }
        }

        let total_flow = buy_flow + sell_flow;
        if total_flow > 0.0 {
            Some((buy_flow - sell_flow) / total_flow)
        } else {
            None
        }
    }

    /// Volume-Weighted Average Trade Price (VWAP)
    pub fn vwap(
        &self,
        trades: &[Trade],
        current_timestamp: Timestamp,
        window_us: i64
    ) -> Option<f64> {
        let cutoff = current_timestamp - window_us;

        let mut total_value = 0.0;
        let mut total_volume = 0.0;

        for trade in trades {
            if trade.timestamp >= cutoff {
                total_value += trade.price * trade.quantity;
                total_volume += trade.quantity;
            }
        }

        if total_volume > 0.0 {
            Some(total_value / total_volume)
        } else {
            None
        }
    }

    /// Trade intensity (trades per second)
    pub fn trade_intensity(
        &self,
        trades: &[Trade],
        current_timestamp: Timestamp,
        window_us: i64
    ) -> Option<f64> {
        let cutoff = current_timestamp - window_us;

        let trade_count = trades
            .iter()
            .filter(|t| t.timestamp >= cutoff)
            .count();

        if trade_count > 0 {
            let window_seconds = window_us as f64 / 1_000_000.0;
            Some(trade_count as f64 / window_seconds)
        } else {
            None
        }
    }

    /// Large trade imbalance
    pub fn large_trade_imbalance(
        &self,
        trades: &[Trade],
        current_timestamp: Timestamp,
        window_us: i64,
        threshold_percentile: f64
    ) -> Option<f64> {
        let cutoff = current_timestamp - window_us;
        let recent_trades: Vec<&Trade> = trades
            .iter()
            .filter(|t| t.timestamp >= cutoff)
            .collect();

        if recent_trades.is_empty() {
            return None;
        }

        // Calculate threshold for large trades
        let mut sizes: Vec<f64> = recent_trades
            .iter()
            .map(|t| t.quantity)
            .collect();
        sizes.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let threshold_idx = ((sizes.len() as f64 * threshold_percentile) as usize)
            .min(sizes.len() - 1);
        let size_threshold = sizes[threshold_idx];

        // Calculate imbalance for large trades
        let mut large_buy_volume = 0.0;
        let mut large_sell_volume = 0.0;

        for trade in recent_trades {
            if trade.quantity >= size_threshold {
                match trade.side {
                    Side::Buy => large_buy_volume += trade.quantity,
                    Side::Sell => large_sell_volume += trade.quantity,
                }
            }
        }

        let total_large_volume = large_buy_volume + large_sell_volume;
        if total_large_volume > 0.0 {
            Some((large_buy_volume - large_sell_volume) / total_large_volume)
        } else {
            None
        }
    }

    /// Momentum based on recent price changes
    pub fn price_momentum(
        &self,
        trades: &[Trade],
        current_timestamp: Timestamp,
        fast_window_us: i64,
        slow_window_us: i64
    ) -> Option<f64> {
        let fast_vwap = self.vwap(trades, current_timestamp, fast_window_us)?;
        let slow_vwap = self.vwap(trades, current_timestamp, slow_window_us)?;

        if slow_vwap > 0.0 {
            Some((fast_vwap - slow_vwap) / slow_vwap)
        } else {
            None
        }
    }
}