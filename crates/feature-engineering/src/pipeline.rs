use matching_engine::MatchingEngine;
use market_types::{OrderBookUpdate, Trade, MarketFeatures, Exchange};
use crate::calculator::FeatureCalculator;
use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::RwLock;
use anyhow::Result;

/// Feature engineering pipeline that connects matching engine to feature calculation
pub struct FeaturePipeline {
    matching_engine: Arc<MatchingEngine>,
    feature_calculators: Arc<RwLock<HashMap<String, FeatureCalculator>>>,
    lookback_ms: i64,
}

impl FeaturePipeline {
    pub fn new(lookback_ms: i64) -> Self {
        Self {
            matching_engine: Arc::new(MatchingEngine::new()),
            feature_calculators: Arc::new(RwLock::new(HashMap::new())),
            lookback_ms,
        }
    }

    /// Process order book update through matching engine and calculate features
    pub fn process_order_book_update(
        &self,
        update: OrderBookUpdate,
    ) -> Result<Option<MarketFeatures>> {
        let symbol = update.symbol.clone();
        let exchange = update.exchange;

        // Process through matching engine to get L3 events
        let _events = self.matching_engine.process_l2_update(&update)?;

        // Get updated order book state
        let book = self.matching_engine.get_order_book(&symbol);

        if let Some(book) = book {
            // Get or create feature calculator for this symbol
            let mut calculators = self.feature_calculators.write();
            let calculator = calculators
                .entry(symbol.clone())
                .or_insert_with(|| FeatureCalculator::new(self.lookback_ms));

            // Update order book history
            calculator.update_order_book(book);

            // Calculate and return features
            Ok(calculator.calculate_features(symbol, exchange))
        } else {
            Ok(None)
        }
    }

    /// Process trade and update feature calculation
    pub fn process_trade(&self, trade: Trade) -> Result<()> {
        let mut calculators = self.feature_calculators.write();
        let calculator = calculators
            .entry(trade.symbol.clone())
            .or_insert_with(|| FeatureCalculator::new(self.lookback_ms));

        calculator.update_trade(trade);
        Ok(())
    }

    /// Get current features for a symbol
    pub fn get_features(&self, symbol: &str, exchange: Exchange) -> Option<MarketFeatures> {
        let calculators = self.feature_calculators.read();
        let calculator = calculators.get(symbol)?;
        calculator.calculate_features(symbol.to_string(), exchange)
    }

    /// Get matching engine reference
    pub fn matching_engine(&self) -> &MatchingEngine {
        &self.matching_engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_types::PriceLevel;

    #[test]
    fn test_pipeline_basic_flow() {
        let pipeline = FeaturePipeline::new(1000);

        let update = OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: "BTCUSDT".to_string(),
            timestamp: 1000000,
            bids: vec![
                PriceLevel { price: 100.0, quantity: 10.0 },
                PriceLevel { price: 99.0, quantity: 5.0 },
            ],
            asks: vec![
                PriceLevel { price: 101.0, quantity: 8.0 },
                PriceLevel { price: 102.0, quantity: 3.0 },
            ],
            is_snapshot: true,
        };

        let features = pipeline.process_order_book_update(update).unwrap();
        assert!(features.is_some());

        let features = features.unwrap();
        assert_eq!(features.symbol, "BTCUSDT");
        assert!(features.spread.is_some());
        assert!(features.order_book_imbalance.is_some());
    }
}
