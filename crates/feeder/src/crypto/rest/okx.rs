use super::*;
use anyhow::Result;
use async_trait::async_trait;

pub struct Okx_RestClient {
    base: BaseRestClient,
}

impl Okx_RestClient {
    pub fn new() -> Self {
        Self {
            base: BaseRestClient::new("https://api.Okx.com", 10),
        }
    }

    pub fn with_credentials(mut self, api_key: String, api_secret: String) -> Self {
        self.base = self.base.with_credentials(api_key, api_secret);
        self
    }
}

#[async_trait]
impl ExchangeRestClient for Okx_RestClient {
    fn name(&self) -> &str {
        "Okx"
    }

    async fn get_orderbook(&self, _symbol: &str, _depth: Option<u32>) -> Result<OrderBookSnapshot> {
        Err(anyhow::anyhow!("Okx orderbook not implemented yet"))
    }

    async fn get_recent_trades(&self, _symbol: &str, _limit: Option<u32>) -> Result<Vec<Trade>> {
        Err(anyhow::anyhow!("Okx trades not implemented yet"))
    }

    async fn get_balances(&self) -> Result<Vec<Balance>> {
        Err(anyhow::anyhow!("Okx balances not implemented yet"))
    }

    async fn place_order(
        &self,
        _symbol: &str,
        _side: OrderSide,
        _order_type: OrderType,
        _quantity: f64,
        _price: Option<f64>,
    ) -> Result<Order> {
        Err(anyhow::anyhow!("Okx order placement not implemented yet"))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Okx order cancellation not implemented yet"))
    }

    async fn get_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Okx order query not implemented yet"))
    }

    async fn get_open_orders(&self, _symbol: Option<&str>) -> Result<Vec<Order>> {
        Err(anyhow::anyhow!("Okx open orders not implemented yet"))
    }
}
