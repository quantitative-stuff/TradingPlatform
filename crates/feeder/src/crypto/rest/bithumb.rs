use super::*;
use anyhow::Result;
use async_trait::async_trait;

pub struct Bithumb_RestClient {
    base: BaseRestClient,
}

impl Bithumb_RestClient {
    pub fn new() -> Self {
        Self {
            base: BaseRestClient::new("https://api.Bithumb.com", 10),
        }
    }

    pub fn with_credentials(mut self, api_key: String, api_secret: String) -> Self {
        self.base = self.base.with_credentials(api_key, api_secret);
        self
    }
}

#[async_trait]
impl ExchangeRestClient for Bithumb_RestClient {
    fn name(&self) -> &str {
        "Bithumb"
    }

    async fn get_orderbook(&self, _symbol: &str, _depth: Option<u32>) -> Result<OrderBookSnapshot> {
        Err(anyhow::anyhow!("Bithumb orderbook not implemented yet"))
    }

    async fn get_recent_trades(&self, _symbol: &str, _limit: Option<u32>) -> Result<Vec<Trade>> {
        Err(anyhow::anyhow!("Bithumb trades not implemented yet"))
    }

    async fn get_balances(&self) -> Result<Vec<Balance>> {
        Err(anyhow::anyhow!("Bithumb balances not implemented yet"))
    }

    async fn place_order(
        &self,
        _symbol: &str,
        _side: OrderSide,
        _order_type: OrderType,
        _quantity: f64,
        _price: Option<f64>,
    ) -> Result<Order> {
        Err(anyhow::anyhow!("Bithumb order placement not implemented yet"))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Bithumb order cancellation not implemented yet"))
    }

    async fn get_order(&self, _symbol: &str, _order_id: &str) -> Result<Order> {
        Err(anyhow::anyhow!("Bithumb order query not implemented yet"))
    }

    async fn get_open_orders(&self, _symbol: Option<&str>) -> Result<Vec<Order>> {
        Err(anyhow::anyhow!("Bithumb open orders not implemented yet"))
    }
}
