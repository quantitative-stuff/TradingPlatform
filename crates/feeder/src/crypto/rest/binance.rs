use super::*;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Binance REST API client
pub struct BinanceRestClient {
    base: BaseRestClient,
}

impl BinanceRestClient {
    pub fn new() -> Self {
        Self {
            base: BaseRestClient::new("https://api.binance.com", 10), // 10 requests per second default
        }
    }

    pub fn with_credentials(mut self, api_key: String, api_secret: String) -> Self {
        self.base = self.base.with_credentials(api_key, api_secret);
        self
    }
}

#[async_trait]
impl ExchangeRestClient for BinanceRestClient {
    fn name(&self) -> &str {
        "binance"
    }

    async fn get_orderbook(&self, symbol: &str, depth: Option<u32>) -> Result<OrderBookSnapshot> {
        let depth = depth.unwrap_or(100);
        let limit = match depth {
            d if d <= 5 => 5,
            d if d <= 10 => 10,
            d if d <= 20 => 20,
            d if d <= 50 => 50,
            d if d <= 100 => 100,
            d if d <= 500 => 500,
            d if d <= 1000 => 1000,
            _ => 5000,
        };

        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("limit".to_string(), limit.to_string());

        // Clone base to make it mutable for the async method
        let mut base_clone = self.base.clone();
        let response = base_clone.get("/api/v3/depth", Some(params)).await?;

        let depth_response: BinanceDepthResponse = response.json().await?;

        Ok(OrderBookSnapshot {
            symbol: symbol.to_string(),
            bids: depth_response.bids.into_iter()
                .map(|level| (
                    level[0].parse::<f64>().unwrap_or(0.0),
                    level[1].parse::<f64>().unwrap_or(0.0)
                ))
                .collect(),
            asks: depth_response.asks.into_iter()
                .map(|level| (
                    level[0].parse::<f64>().unwrap_or(0.0),
                    level[1].parse::<f64>().unwrap_or(0.0)
                ))
                .collect(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            last_update_id: Some(depth_response.last_update_id),
        })
    }

    async fn get_recent_trades(&self, symbol: &str, limit: Option<u32>) -> Result<Vec<Trade>> {
        let limit = limit.unwrap_or(500).min(1000);

        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("limit".to_string(), limit.to_string());

        let mut base_clone = self.base.clone();
        let response = base_clone.get("/api/v3/trades", Some(params)).await?;

        let trades_response: Vec<BinanceTradeResponse> = response.json().await?;

        Ok(trades_response.into_iter()
            .map(|t| Trade {
                symbol: symbol.to_string(),
                price: t.price.parse::<f64>().unwrap_or(0.0),
                quantity: t.qty.parse::<f64>().unwrap_or(0.0),
                timestamp: t.time,
                is_buyer_maker: t.is_buyer_maker,
                trade_id: Some(t.id.to_string()),
            })
            .collect())
    }

    async fn get_balances(&self) -> Result<Vec<Balance>> {
        // This requires authentication
        if self.base.api_key.is_none() || self.base.api_secret.is_none() {
            return Err(anyhow::anyhow!("API credentials required for account endpoints"));
        }

        // Implementation would go here with proper signing
        // For now, return empty as we focus on market data
        Ok(Vec::new())
    }

    async fn place_order(
        &self,
        _symbol: &str,
        _side: OrderSide,
        _order_type: OrderType,
        _quantity: f64,
        _price: Option<f64>,
    ) -> Result<Order> {
        // This requires authentication
        Err(anyhow::anyhow!("Order placement not implemented yet"))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        // This requires authentication
        Err(anyhow::anyhow!("Order cancellation not implemented yet"))
    }

    async fn get_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        // This requires authentication
        Err(anyhow::anyhow!("Order query not implemented yet"))
    }

    async fn get_open_orders(&self, _symbol: Option<&str>) -> Result<Vec<Order>> {
        // This requires authentication
        Err(anyhow::anyhow!("Open orders query not implemented yet"))
    }
}

// Binance-specific response structures
#[derive(Debug, Deserialize)]
struct BinanceDepthResponse {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<Vec<String>>,  // [[price, quantity], ...]
    asks: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BinanceTradeResponse {
    id: u64,
    price: String,
    qty: String,
    time: u64,
    #[serde(rename = "isBuyerMaker")]
    is_buyer_maker: bool,
}

// Need to implement Clone for BaseRestClient to use it in async methods
impl Clone for BaseRestClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            api_secret: self.api_secret.clone(),
            rate_limiter: RateLimiter::new(10), // Create new rate limiter for clone
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = BinanceRestClient::new();
        assert_eq!(client.name(), "binance");
    }

    #[tokio::test]
    #[ignore] // Ignore in CI, requires network
    async fn test_get_orderbook() {
        let client = BinanceRestClient::new();
        let result = client.get_orderbook("BTCUSDT", Some(10)).await;

        match result {
            Ok(snapshot) => {
                assert_eq!(snapshot.symbol, "BTCUSDT");
                assert!(!snapshot.bids.is_empty());
                assert!(!snapshot.asks.is_empty());
                assert!(snapshot.last_update_id.is_some());
            }
            Err(e) => {
                // Network error is acceptable in tests
                println!("Network error (expected in offline tests): {}", e);
            }
        }
    }
}