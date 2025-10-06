use std::sync::Arc;
use crate::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    async fn get_market_price(&self, symbol: &str) -> Result<f64>;
}

/// Generic market price function that works with any MarketDataProvider
pub async fn get_market_price(provider: &Arc<dyn MarketDataProvider>, symbol: &str) -> Result<f64> {
    provider.get_market_price(symbol).await
}

// /// Mock implementation for testing
// pub struct MockMarketDataProvider;

// #[async_trait]
// impl MarketDataProvider for MockMarketDataProvider {
//     async fn get_market_price(&self, _symbol: &str) -> Result<f64> {
//         use rand::Rng;
//         let mut rng = rand::thread_rng();
//         let price = 30000.0 + (rng.gen::<f64>() * 1000.0 - 500.0);
//         Ok(price)
//     }
// }