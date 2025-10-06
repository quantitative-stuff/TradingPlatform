use crate::core::MarketDataProvider;
use crate::error::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

pub struct BinanceMarketDataProvider {
    pub client: Client,
}

impl BinanceMarketDataProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl MarketDataProvider for BinanceMarketDataProvider {
    async fn get_market_price(&self, symbol: &str) -> Result<f64> {
        let url = format!("https://api.binance.com/api/v3/ticker/price?symbol={}", symbol);
        
        let response = self.client.get(&url).send().await?;
        let data: Value = response.json().await?;
        
        if let Some(price_str) = data.get("price").and_then(|p| p.as_str()) {
            let price = price_str.parse::<f64>()
                .map_err(|_| crate::error::Error::InvalidInput("Failed to parse price".into()))?;
            Ok(price)
        } else {
            Err(crate::error::Error::InvalidInput("Price not found in response".into()))
        }
    }
}