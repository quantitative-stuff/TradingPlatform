pub mod udp_receiver;
pub mod historical_loader;
pub mod data_validator;

use crate::types::{OrderBookUpdate, Trade, Timestamp};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Unified data source trait for both live and historical data
#[async_trait]
pub trait DataSource: Send + Sync {
    async fn start(&mut self) -> Result<()>;
    async fn stop(&mut self) -> Result<()>;
    fn orderbook_channel(&self) -> mpsc::Receiver<OrderBookUpdate>;
    fn trades_channel(&self) -> mpsc::Receiver<Trade>;
}

/// Data validation trait
pub trait DataValidator {
    fn validate_orderbook(&self, update: &OrderBookUpdate) -> Result<()>;
    fn validate_trade(&self, trade: &Trade) -> Result<()>;
}