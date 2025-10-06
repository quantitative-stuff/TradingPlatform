use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{warn, debug};

use crate::error::Result;
use crate::core::{MarketData, MarketDataType};
use super::{QuestDBClient, DatabaseType, DatabaseWriter};

#[derive(Debug, Clone)]
pub enum DataFlow {
    Crypto,
    Stock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub crypto_routing: RoutingRules,
    pub stock_routing: RoutingRules,
    pub enable_filtering: bool,
    pub enable_aggregation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRules {
    pub trade_destination: DatabaseType,
    pub orderbook_destination: DatabaseType,
    pub metadata_destination: DatabaseType,
    pub enable_deduplication: bool,
    pub batch_size: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            crypto_routing: RoutingRules {
                trade_destination: DatabaseType::QuestDB,
                orderbook_destination: DatabaseType::QuestDB,
                metadata_destination: DatabaseType::QuestDB,
                enable_deduplication: true,
                batch_size: 100,
            },
            stock_routing: RoutingRules {
                trade_destination: DatabaseType::QuestDB,
                orderbook_destination: DatabaseType::QuestDB,
                metadata_destination: DatabaseType::QuestDB,
                enable_deduplication: true,
                batch_size: 50,
            },
            enable_filtering: true,
            enable_aggregation: false,
        }
    }
}

pub struct DataRouter {
    config: RouterConfig,
    questdb_client: Arc<RwLock<QuestDBClient>>,
    routing_stats: Arc<RwLock<RoutingStats>>,
}

#[derive(Debug, Default, Clone)]
struct RoutingStats {
    crypto_trades_routed: u64,
    crypto_orderbooks_routed: u64,
    crypto_metadata_routed: u64,
    stock_trades_routed: u64,
    stock_orderbooks_routed: u64,
    stock_metadata_routed: u64,
    duplicates_filtered: u64,
    routing_errors: u64,
}

impl DataRouter {
    pub fn new(
        config: RouterConfig,
        questdb_client: QuestDBClient,
    ) -> Self {
        Self {
            config,
            questdb_client: Arc::new(RwLock::new(questdb_client)),
            routing_stats: Arc::new(RwLock::new(RoutingStats::default())),
        }
    }

    pub async fn route_market_data(&self, data: MarketData, flow: DataFlow) -> Result<()> {
        match flow {
            DataFlow::Crypto => self.route_crypto_data(data).await,
            DataFlow::Stock => self.route_stock_data(data).await,
        }
    }

    async fn route_crypto_data(&self, data: MarketData) -> Result<()> {
        let rules = &self.config.crypto_routing;
        
        match data.data_type {
            MarketDataType::Trade(trade) => {
                match rules.trade_destination {
                    DatabaseType::QuestDB => {
                        let client = self.questdb_client.write().await;
                        // TODO: Implement feeder log methods
                        // client.write_crypto_trade(&trade).await?;
                        self.routing_stats.write().await.crypto_trades_routed += 1;
                    }
                    DatabaseType::KdbPlus => {
                        warn!("KdbPlus is not supported for crypto trades yet");
                    }
                }
            }
            MarketDataType::OrderBook(orderbook) => {
                match rules.orderbook_destination {
                    DatabaseType::QuestDB => {
                        let client = self.questdb_client.write().await;
                        // TODO: Implement feeder log methods
                        // client.write_crypto_orderbook(&orderbook).await?;
                        self.routing_stats.write().await.crypto_orderbooks_routed += 1;
                    }
                    DatabaseType::KdbPlus => {
                        warn!("KdbPlus is not supported for crypto orderbooks yet");
                    }
                }
            }
            _ => {
                debug!("Unsupported market data type for crypto flow");
            }
        }
        
        Ok(())
    }

    async fn route_stock_data(&self, data: MarketData) -> Result<()> {
        let rules = &self.config.stock_routing;
        
        match data.data_type {
            MarketDataType::Trade(trade) => {
                match rules.trade_destination {
                    DatabaseType::QuestDB => {
                        let client = self.questdb_client.write().await;
                        // TODO: Implement feeder log methods
                        // client.write_stock_trade(&trade).await?;
                        self.routing_stats.write().await.stock_trades_routed += 1;
                    }
                    DatabaseType::KdbPlus => {
                        warn!("KdbPlus is not supported for stock trades yet");
                    }
                }
            }
            MarketDataType::OrderBook(orderbook) => {
                match rules.orderbook_destination {
                    DatabaseType::QuestDB => {
                        let client = self.questdb_client.write().await;
                        // TODO: Implement feeder log methods
                        // client.write_stock_orderbook(&orderbook).await?;
                        self.routing_stats.write().await.stock_orderbooks_routed += 1;
                    }
                    DatabaseType::KdbPlus => {
                        warn!("KdbPlus is not supported for stock orderbooks yet");
                    }
                }
            }
            _ => {
                debug!("Unsupported market data type for stock flow");
            }
        }
        
        Ok(())
    }

    pub async fn route_metadata(&self, metadata: Vec<u8>, flow: DataFlow) -> Result<()> {
        let rules = match flow {
            DataFlow::Crypto => &self.config.crypto_routing,
            DataFlow::Stock => &self.config.stock_routing,
        };

        match rules.metadata_destination {
            DatabaseType::QuestDB => {
                let mut client = self.questdb_client.write().await;
                client.write_metadata(&metadata).await?;

                match flow {
                    DataFlow::Crypto => {
                        self.routing_stats.write().await.crypto_metadata_routed += 1;
                    }
                    DataFlow::Stock => {
                        self.routing_stats.write().await.stock_metadata_routed += 1;
                    }
                }
            }
            DatabaseType::KdbPlus => {
                warn!("Metadata routing to KdbPlus not supported yet");
            }
        }

        Ok(())
    }

    pub async fn get_stats(&self) -> RoutingStats {
        self.routing_stats.read().await.clone()
    }

    pub async fn reset_stats(&self) {
        *self.routing_stats.write().await = RoutingStats::default();
    }
}

// Standalone data flow managers for crypto and stock
pub struct CryptoDataFlow {
    router: Arc<DataRouter>,
    config: CryptoFlowConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoFlowConfig {
    pub exchanges: Vec<String>,
    pub symbols: Vec<String>,
    pub enable_all_symbols: bool,
    pub data_types: Vec<String>,
}

impl CryptoDataFlow {
    pub fn new(router: Arc<DataRouter>, config: CryptoFlowConfig) -> Self {
        Self { router, config }
    }

    pub async fn process(&self, data: MarketData) -> Result<()> {
        // Apply filtering based on config
        if !self.config.enable_all_symbols && !self.config.symbols.contains(&data.symbol) {
            return Ok(());
        }
        
        if !self.config.exchanges.is_empty() && !self.config.exchanges.contains(&data.exchange) {
            return Ok(());
        }
        
        self.router.route_market_data(data, DataFlow::Crypto).await
    }
}

pub struct StockDataFlow {
    router: Arc<DataRouter>,
    config: StockFlowConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockFlowConfig {
    pub exchanges: Vec<String>,
    pub symbols: Vec<String>,
    pub asset_types: Vec<String>,
    pub enable_derivatives: bool,
    pub enable_etf: bool,
}

impl StockDataFlow {
    pub fn new(router: Arc<DataRouter>, config: StockFlowConfig) -> Self {
        Self { router, config }
    }

    pub async fn process(&self, data: MarketData) -> Result<()> {
        // Apply filtering based on config
        if !self.config.symbols.is_empty() && !self.config.symbols.contains(&data.symbol) {
            return Ok(());
        }
        
        if !self.config.exchanges.is_empty() && !self.config.exchanges.contains(&data.exchange) {
            return Ok(());
        }
        
        // Check asset type filtering
        let asset_type = match &data.data_type {
            MarketDataType::Trade(t) => &t.asset_type,
            MarketDataType::OrderBook(o) => &o.asset_type,
            _ => return Ok(()),
        };
        
        if !self.config.enable_derivatives && 
           (asset_type == "futures" || asset_type == "options") {
            return Ok(());
        }
        
        if !self.config.enable_etf && asset_type == "etf" {
            return Ok(());
        }
        
        if !self.config.asset_types.is_empty() && 
           !self.config.asset_types.contains(asset_type) {
            return Ok(());
        }
        
        self.router.route_market_data(data, DataFlow::Stock).await
    }
}
