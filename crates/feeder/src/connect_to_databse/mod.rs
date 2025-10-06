pub mod questdb;
pub mod kdbplus;
pub mod udp_receiver;
pub mod data_router;
pub mod log_writer;
pub mod feeder_logger;
pub mod questdb_log_layer;

pub use questdb::{QuestDBClient, QuestDBConfig};
pub use kdbplus::{KdbClient, KdbConfig, init_kdb_client};
pub use udp_receiver::{DatabaseUDPReceiver, ReceiverConfig};
pub use data_router::{DataRouter, DataFlow};
pub use log_writer::{LogWriter, LogEntry, LogMetrics};
pub use feeder_logger::{
    FeederLogger, ConnectionEvent, PerformanceMetrics,
    ConnectionStats, ErrorLog, ProcessingStats, SystemMetricsCollector
};
pub use questdb_log_layer::QuestDBLogLayer;

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait DatabaseWriter {
    async fn write_trade(&mut self, data: &[u8]) -> Result<()>;
    async fn write_orderbook(&mut self, data: &[u8]) -> Result<()>;
    async fn write_metadata(&mut self, data: &[u8]) -> Result<()>;
    async fn flush(&mut self) -> Result<()>;
    async fn health_check(&self) -> Result<bool>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatabaseType {
    QuestDB,
    KdbPlus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataFrequency {
    High,  // Real-time tick data (WebSocket)
    Mid,   // REST API data
}