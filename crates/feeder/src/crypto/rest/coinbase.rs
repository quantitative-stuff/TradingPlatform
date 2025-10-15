use super::*;
use anyhow::Result;
use async_trait::async_trait;

pub struct Coinbase_RestClient {
    base: BaseRestClient,
}

impl Coinbase_RestClient {
    pub fn new() -> Self {
        Self {
            base: BaseRestClient::new("https://api.Coinbase.com", 10),
        }
    }

    pub fn with_credentials(mut self, api_key: String, api_secret: String) -> Self {
        self.base = self.base.with_credentials(api_key, api_secret);
        self
    }
}

#[async_trait]
impl ExchangeRestClient for Coinbase_RestClient {
    fn name(&self) -> &str {
        "Coinbase"
    }

    async fn get_orderbook(&self, _symbol: &str, _depth: Option<u32>) -> Result<OrderBookSnapshot> {
        Err(anyhow::anyhow!("Coinbase orderbook not implemented yet"))
    }

    async fn get_recent_trades(&self, _symbol: &str, _limit: Option<u32>) -> Result<Vec<Trade>> {
        Err(anyhow::anyhow!("Coinbase trades not implemented yet"))
    }

    async fn get_balances(&self) -> Result<Vec<Balance>> {
        Err(anyhow::anyhow!("Coinbase balances not implemented yet"))
    }

    async fn place_order(
        &self,
        _symbol: &str,
        _side: OrderSide,
        _order_type: OrderType,
        _quantity: f64,
        _price: Option<f64>,
    ) -> Result<Order> {
        Err(anyhow::anyhow!("Coinbase order placement not implemented yet"))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Coinbase order cancellation not implemented yet"))
    }

    async fn get_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Coinbase order query not implemented yet"))
    }

    async fn get_open_orders(&self, _symbol: Option<&str>) -> Result<Vec<Order>> {
        Err(anyhow::anyhow!("Coinbase open orders not implemented yet"))
    }
}
