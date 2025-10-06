use async_trait::async_trait;
use crate::error::Result;
use crate::core::types::MarketData;


#[async_trait]
pub trait Feeder: Send + Sync {
    fn name(&self) -> &str;
    async fn connect(&mut self) -> Result<()>;
    async fn subscribe(&mut self) -> Result<()>;
    async fn start(&mut self) -> Result<()>;
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn store(&self, data: &MarketData) -> Result<()>;
}

/// The TradeComparator trait provides a common interface for comparing market data,
/// such as trades or order books, across different exchanges.
pub trait TradeComparator: Send + Sync {
    fn compare_trades(&self, symbol: &str);
    fn compare_orderbooks(&self, symbol: &str);
}

