use crate::types::{OrderBook, Timestamp};

pub struct WAPCalculator {
    default_depth_bps: f64,
}

impl WAPCalculator {
    pub fn new() -> Self {
        Self {
            default_depth_bps: 10.0, // 10 basis points default
        }
    }

    /// Calculate WAP for current order book
    pub fn calculate(&self, book: &OrderBook, depth_bps: f64) -> Option<f64> {
        let depth = if depth_bps > 0.0 { depth_bps } else { self.default_depth_bps };
        book.wap(depth)
    }

    /// Calculate WAP at a specific timestamp from historical data
    pub fn calculate_lagged(
        &self,
        history: &[OrderBook],
        target_timestamp: Timestamp,
        depth_bps: f64
    ) -> Option<f64> {
        // Find the book closest to target timestamp
        let book = history
            .iter()
            .filter(|b| b.timestamp <= target_timestamp)
            .max_by_key(|b| b.timestamp)?;

        self.calculate(book, depth_bps)
    }

    /// Calculate momentum between two WAP values
    pub fn calculate_momentum(
        &self,
        current_wap: Option<f64>,
        past_wap: Option<f64>
    ) -> Option<f64> {
        match (current_wap, past_wap) {
            (Some(current), Some(past)) if past > 0.0 => {
                Some((current - past) / past)
            }
            _ => None
        }
    }

    /// Calculate rolling WAP statistics
    pub fn calculate_rolling_stats(
        &self,
        history: &[OrderBook],
        window_size_us: i64,
        depth_bps: f64
    ) -> WAPStats {
        if history.is_empty() {
            return WAPStats::default();
        }

        let latest_timestamp = history.last().unwrap().timestamp;
        let cutoff_timestamp = latest_timestamp - window_size_us;

        let wap_values: Vec<f64> = history
            .iter()
            .filter(|b| b.timestamp >= cutoff_timestamp)
            .filter_map(|b| self.calculate(b, depth_bps))
            .collect();

        if wap_values.is_empty() {
            return WAPStats::default();
        }

        let mean = wap_values.iter().sum::<f64>() / wap_values.len() as f64;

        let variance = wap_values
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / wap_values.len() as f64;

        let std_dev = variance.sqrt();

        let min = wap_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = wap_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        WAPStats {
            mean: Some(mean),
            std_dev: Some(std_dev),
            min: Some(min),
            max: Some(max),
            count: wap_values.len(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WAPStats {
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub count: usize,
}

impl WAPStats {
    pub fn z_score(&self, value: f64) -> Option<f64> {
        match (self.mean, self.std_dev) {
            (Some(mean), Some(std)) if std > 0.0 => {
                Some((value - mean) / std)
            }
            _ => None
        }
    }
}