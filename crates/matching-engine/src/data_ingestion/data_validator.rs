use crate::types::{OrderBookUpdate, Trade, Timestamp};
use anyhow::{Result, bail};
use std::collections::HashMap;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct DataValidator {
    last_timestamps: Arc<RwLock<HashMap<String, Timestamp>>>,
    last_prices: Arc<RwLock<HashMap<String, f64>>>,
    max_price_change_pct: f64,
    max_stale_ms: i64,
}

impl DataValidator {
    pub fn new(max_price_change_pct: f64, max_stale_ms: i64) -> Self {
        Self {
            last_timestamps: Arc::new(RwLock::new(HashMap::new())),
            last_prices: Arc::new(RwLock::new(HashMap::new())),
            max_price_change_pct,
            max_stale_ms,
        }
    }

    pub fn validate_orderbook(&self, update: &OrderBookUpdate) -> Result<()> {
        // Check timestamp monotonicity
        self.check_timestamp_monotonic(&update.symbol, update.timestamp)?;

        // Check for stale data
        self.check_staleness(update.timestamp)?;

        // Validate price levels
        self.validate_price_levels(&update.bids, &update.asks)?;

        // Check for crossed markets
        if !update.bids.is_empty() && !update.asks.is_empty() {
            let best_bid = update.bids[0].price;
            let best_ask = update.asks[0].price;

            if best_bid >= best_ask {
                bail!("Crossed market detected: bid {} >= ask {}", best_bid, best_ask);
            }
        }

        // Check for outlier prices
        if let Some(mid_price) = self.calculate_mid_price(update) {
            self.check_price_outlier(&update.symbol, mid_price)?;
        }

        Ok(())
    }

    pub fn validate_trade(&self, trade: &Trade) -> Result<()> {
        // Check timestamp
        self.check_timestamp_monotonic(&trade.symbol, trade.timestamp)?;
        self.check_staleness(trade.timestamp)?;

        // Validate price
        if trade.price <= 0.0 {
            bail!("Invalid trade price: {}", trade.price);
        }

        // Validate quantity
        if trade.quantity <= 0.0 {
            bail!("Invalid trade quantity: {}", trade.quantity);
        }

        // Check for price outliers
        self.check_price_outlier(&trade.symbol, trade.price)?;

        Ok(())
    }

    fn check_timestamp_monotonic(&self, symbol: &str, timestamp: Timestamp) -> Result<()> {
        let mut timestamps = self.last_timestamps.write();

        if let Some(&last_ts) = timestamps.get(symbol) {
            if timestamp < last_ts {
                bail!(
                    "Non-monotonic timestamp for {}: {} < {}",
                    symbol, timestamp, last_ts
                );
            }
        }

        timestamps.insert(symbol.to_string(), timestamp);
        Ok(())
    }

    fn check_staleness(&self, timestamp: Timestamp) -> Result<()> {
        let current_time = chrono::Utc::now().timestamp_micros();
        let age_ms = (current_time - timestamp) / 1000;

        if age_ms > self.max_stale_ms {
            bail!("Stale data: {} ms old", age_ms);
        }

        Ok(())
    }

    fn validate_price_levels(
        &self,
        bids: &[crate::types::PriceLevel],
        asks: &[crate::types::PriceLevel],
    ) -> Result<()> {
        // Check bids are sorted descending
        for i in 1..bids.len() {
            if bids[i].price >= bids[i - 1].price {
                bail!("Bids not sorted descending");
            }
        }

        // Check asks are sorted ascending
        for i in 1..asks.len() {
            if asks[i].price <= asks[i - 1].price {
                bail!("Asks not sorted ascending");
            }
        }

        // Check for zero or negative prices
        for level in bids.iter().chain(asks.iter()) {
            if level.price <= 0.0 {
                bail!("Invalid price: {}", level.price);
            }
            if level.quantity < 0.0 {
                bail!("Invalid quantity: {}", level.quantity);
            }
        }

        Ok(())
    }

    fn check_price_outlier(&self, symbol: &str, price: f64) -> Result<()> {
        let mut last_prices = self.last_prices.write();

        if let Some(&last_price) = last_prices.get(symbol) {
            let change_pct = ((price - last_price) / last_price).abs() * 100.0;

            if change_pct > self.max_price_change_pct {
                bail!(
                    "Price outlier detected for {}: {:.2}% change",
                    symbol, change_pct
                );
            }
        }

        last_prices.insert(symbol.to_string(), price);
        Ok(())
    }

    fn calculate_mid_price(&self, update: &OrderBookUpdate) -> Option<f64> {
        if !update.bids.is_empty() && !update.asks.is_empty() {
            Some((update.bids[0].price + update.asks[0].price) / 2.0)
        } else {
            None
        }
    }

    pub fn reset(&self) {
        self.last_timestamps.write().clear();
        self.last_prices.write().clear();
    }
}