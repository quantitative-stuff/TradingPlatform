use super::*;
use anyhow::Result;
use async_trait::async_trait;

/// Bybit REST API client
pub struct BybitRestClient {
    base: BaseRestClient,
}

impl BybitRestClient {
    pub fn new() -> Self {
        Self {
            base: BaseRestClient::new("https://api.bybit.com", 10),
        }
    }

    pub fn with_credentials(mut self, api_key: String, api_secret: String) -> Self {
        self.base = self.base.with_credentials(api_key, api_secret);
        self
    }
}

#[async_trait]
impl ExchangeRestClient for BybitRestClient {
    fn name(&self) -> &str {
        "bybit"
    }

    async fn get_orderbook(&self, _symbol: &str, _depth: Option<u32>) -> Result<OrderBookSnapshot> {
        // TODO: Implement Bybit orderbook snapshot
        Err(anyhow::anyhow!("Bybit orderbook not implemented yet"))
    }

    async fn get_recent_trades(&self, _symbol: &str, _limit: Option<u32>) -> Result<Vec<Trade>> {
        Err(anyhow::anyhow!("Bybit trades not implemented yet"))
    }

    async fn get_balances(&self) -> Result<Vec<Balance>> {
        Err(anyhow::anyhow!("Bybit balances not implemented yet"))
    }

    async fn place_order(
        &self,
        _symbol: &str,
        _side: OrderSide,
        _order_type: OrderType,
        _quantity: f64,
        _price: Option<f64>,
    ) -> Result<Order> {
        Err(anyhow::anyhow!("Bybit order placement not implemented yet"))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Bybit order cancellation not implemented yet"))
    }

    async fn get_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Bybit order query not implemented yet"))
    }

    async fn get_open_orders(&self, _symbol: Option<&str>) -> Result<Vec<Order>> {
        Err(anyhow::anyhow!("Bybit open orders not implemented yet"))
    }
}

impl Clone for BybitRestClient {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
        }
    }
}