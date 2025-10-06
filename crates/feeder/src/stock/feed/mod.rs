// mod ebest;

pub mod ls_ws;
pub mod ls_rest;
pub mod ls_enhanced;

pub use ls_ws::LSExchange;
pub use ls_rest::LSClient;
pub use ls_enhanced::{LSEnhancedExchange, AssetType, OptionsData, FuturesData, ETFData, DerivativeInfo};

use async_trait::async_trait;
use crate::error::Result;

#[async_trait]
pub trait FeederStock {
    fn name(&self) -> &str;
    async fn connect(&mut self) -> Result<()>;
    async fn subscribe(&mut self) -> Result<()>;
    async fn start(&mut self) -> Result<()>;
}

