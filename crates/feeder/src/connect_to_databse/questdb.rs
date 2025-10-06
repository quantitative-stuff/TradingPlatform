use async_trait::async_trait;
use tokio::net::UdpSocket;
use std::net::SocketAddr;
use serde::{Deserialize, Serialize};
use tracing::{info, error, debug};
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

use crate::error::{Result, Error};
use super::DatabaseWriter;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuestDBConfig {
    pub host: String,
    pub ilp_port: u16,  // InfluxDB Line Protocol port (usually 9009)
    pub http_port: u16,  // HTTP port for queries (usually 9000)
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub feeder_tables: FeederLogTables,
    pub config_tables: ConfigTables,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeederLogTables {
    pub connection_logs: String,
    pub performance_logs: String,
    pub error_logs: String,
    pub stats_logs: String,
    pub full_logs: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfigTables {
    pub exchange_configs: String,
    pub symbol_mappings: String,
    pub connection_templates: String,
    pub asset_specifications: String,
}

impl Default for QuestDBConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            ilp_port: 9009,
            http_port: 9000,
            batch_size: 1000,
            flush_interval_ms: 100,
            feeder_tables: FeederLogTables {
                connection_logs: "feeder_connection_logs".to_string(),
                performance_logs: "feeder_performance_logs".to_string(),
                error_logs: "feeder_error_logs".to_string(),
                stats_logs: "feeder_stats_logs".to_string(),
                full_logs: "feeder_full_logs".to_string(),
            },
            config_tables: ConfigTables {
                exchange_configs: "exchange_configs".to_string(),
                symbol_mappings: "symbol_mappings".to_string(),
                connection_templates: "connection_templates".to_string(),
                asset_specifications: "asset_specifications".to_string(),
            },
        }
    }
}

#[derive(Clone)]
pub struct QuestDBClient {
    config: QuestDBConfig,
    socket: Arc<UdpSocket>,
    server_addr: SocketAddr,
    buffer: Arc<RwLock<Vec<String>>>,
    last_flush: Arc<RwLock<DateTime<Utc>>>,
}

impl QuestDBClient {
    pub async fn new(config: QuestDBConfig) -> Result<Self> {
        let addr = format!("{}:{}", config.host, config.ilp_port);
        let server_addr: SocketAddr = addr.parse()
            .map_err(|e| Error::Connection(format!("Invalid QuestDB address: {}", e)))?;
        
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(&server_addr).await?;
        
        info!("QuestDB client initialized for {}", addr);
        
        Ok(Self {
            config,
            socket: Arc::new(socket),
            server_addr,
            buffer: Arc::new(RwLock::new(Vec::with_capacity(1000))),
            last_flush: Arc::new(RwLock::new(Utc::now())),
        })
    }

    pub async fn write_connection_log(
        &self, 
        exchange: &str,
        connection_id: &str,
        event_type: &str,
        status: &str,
        error_msg: Option<&str>,
    ) -> Result<()> {
        self.write_connection_log_with_attempt(exchange, connection_id, event_type, status, error_msg, None).await
    }

    pub async fn write_full_log(
        &self,
        level: &str,
        target: &str,
        thread_id: &str,
        message: &str,
    ) -> Result<()> {
        // Maximum UDP packet size limit (conservative limit for network transmission)
        const MAX_UDP_PACKET_SIZE: usize = 1200; // Leave room for headers
        const MAX_MESSAGE_SIZE: usize = 800; // Leave room for other fields
        
        // Escape special characters in the message and truncate if too long
        let mut message_escaped = message.replace('"', "\\\"").replace('\n', "\\n").replace('\r', "");
        
        // Truncate message if it's too long
        if message_escaped.len() > MAX_MESSAGE_SIZE {
            message_escaped = format!("{}...[TRUNCATED_{}chars]", 
                &message_escaped[..MAX_MESSAGE_SIZE-50], 
                message.len());
        }
        
        let target_escaped = target.replace(' ', "_").replace(',', "_");
        let level_escaped = level.replace(' ', "_").replace(',', "_");
        let thread_id_escaped = thread_id.replace(' ', "_").replace(',', "_");
        
        let line = format!(
            "{},level={},target={},thread_id={} message=\"{}\" {}",
            self.config.feeder_tables.full_logs,
            level_escaped,
            target_escaped,
            thread_id_escaped,
            message_escaped,
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        
        // Double-check total packet size and truncate further if needed
        if line.len() > MAX_UDP_PACKET_SIZE {
            let excess = line.len() - MAX_UDP_PACKET_SIZE + 100; // Extra safety margin
            if message_escaped.len() > excess {
                let truncated_message = format!("{}...[TRUNCATED_PACKET]", 
                    &message_escaped[..message_escaped.len()-excess]);
                let line = format!(
                    "{},level={},target={},thread_id={} message=\"{}\" {}",
                    self.config.feeder_tables.full_logs,
                    level_escaped,
                    target_escaped,
                    thread_id_escaped,
                    truncated_message,
                    Utc::now().timestamp_nanos_opt().unwrap_or(0)
                );
                debug!("Full log ILP line (truncated): {}", line);
                return self.add_to_buffer(line).await;
            }
        }
        
        debug!("Full log ILP line: {}", line);
        self.add_to_buffer(line).await
    }

    pub async fn write_connection_log_with_attempt(
        &self, 
        exchange: &str,
        connection_id: &str,
        event_type: &str,
        status: &str,
        error_msg: Option<&str>,
        attempt: Option<u32>,
    ) -> Result<()> {
        // Maximum message size for error messages to prevent UDP packet size issues
        const MAX_ERROR_MSG_SIZE: usize = 500;
        
        // ILP format: table,tag1=value1,tag2=value2 field1=value1,field2="string value" timestamp_nanos
        let mut error_msg_escaped = error_msg.unwrap_or("").replace('"', "\\\"").replace('\n', "\\n");
        
        // Truncate error message if too long to prevent OS error 10040
        if error_msg_escaped.len() > MAX_ERROR_MSG_SIZE {
            error_msg_escaped = format!("{}...[TRUNCATED_{}chars]", 
                &error_msg_escaped[..MAX_ERROR_MSG_SIZE-30], 
                error_msg.unwrap_or("").len());
        }
        
        let line = format!(
            "{},exchange={},connection_id={},event_type={} status=\"{}\",error_msg=\"{}\",attempt={}i {}",
            self.config.feeder_tables.connection_logs,
            exchange.replace(' ', "_").replace(',', "_"),
            connection_id.replace(' ', "_").replace(',', "_"),
            event_type.replace(' ', "_").replace(',', "_"),
            status,
            error_msg_escaped,
            attempt.unwrap_or(1),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        
        debug!("ILP line: {}", line);
        self.add_to_buffer(line).await
    }

    pub async fn write_performance_log(
        &self,
        exchange: &str,
        cpu_usage: f64,
        memory_mb: f64,
        websocket_count: u32,
        data_rate: f64,
        latency_ms: f64,
    ) -> Result<()> {
        // ILP format: measurement,tag=value field1=value1,field2=value2 timestamp
        let line = format!(
            "{},exchange={} cpu_usage={},memory_mb={},websocket_count={}i,data_rate={},latency_ms={} {}",
            self.config.feeder_tables.performance_logs,
            exchange.replace(' ', "_").replace(',', "_"),
            cpu_usage,
            memory_mb,
            websocket_count,
            data_rate,
            latency_ms,
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        
        debug!("ILP line: {}", line);
        self.add_to_buffer(line).await
    }

    pub async fn write_error_log(
        &self,
        level: &str,
        module: &str,
        error_code: Option<&str>,
        message: &str,
        context: Option<&str>,
    ) -> Result<()> {
        let message_escaped = message.replace('"', "\\\"").replace('\n', "\\n");
        let context_escaped = context.unwrap_or("").replace('"', "\\\"").replace('\n', "\\n");
        let error_code_escaped = error_code.unwrap_or("").replace('"', "\\\"");
        
        let line = format!(
            "{},level={},module={} error_code=\"{}\",message=\"{}\",context=\"{}\" {}",
            self.config.feeder_tables.error_logs,
            level.replace(' ', "_").replace(',', "_"),
            module.replace(' ', "_").replace(',', "_"),
            error_code_escaped,
            message_escaped,
            context_escaped,
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        
        debug!("ILP line: {}", line);
        self.add_to_buffer(line).await
    }

    pub async fn write_stats_log(
        &self,
        exchange: &str,
        data_type: &str,
        messages_received: u64,
        messages_processed: u64,
        dropped_count: u64,
    ) -> Result<()> {
        // Use 'i' suffix for integer values in ILP
        let line = format!(
            "{},exchange={},data_type={} messages_received={}i,messages_processed={}i,dropped_count={}i {}",
            self.config.feeder_tables.stats_logs,
            exchange.replace(' ', "_").replace(',', "_"),
            data_type.replace(' ', "_").replace(',', "_"),
            messages_received,
            messages_processed,
            dropped_count,
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        
        debug!("ILP line: {}", line);
        self.add_to_buffer(line).await
    }

    // Market data methods - deprecated, use feeder log methods instead
    /*
    pub async fn write_options_data(&self, symbol: &str, data: &crate::adapter_feed_stock::ls_enhanced::OptionsData) -> Result<()> {
        let line = format!(
            "{},symbol={},underlying={},type={},strike={} price={},iv={},delta={},gamma={},theta={},vega={},oi={},volume={} {}",
            self.config.stock_tables.options,
            symbol,
            data.underlying,
            match data.option_type {
                crate::adapter_feed_stock::ls_enhanced::OptionType::Call => "C",
                crate::adapter_feed_stock::ls_enhanced::OptionType::Put => "P",
            },
            data.strike_price,
            data.price,
            data.implied_volatility,
            data.greeks.delta,
            data.greeks.gamma,
            data.greeks.theta,
            data.greeks.vega,
            data.open_interest,
            data.volume,
            data.timestamp * 1_000_000
        );
        
        self.add_to_buffer(line).await
    }
    */

    /*
    pub async fn write_futures_data(&self, symbol: &str, data: &crate::adapter_feed_stock::ls_enhanced::FuturesData) -> Result<()> {
        let line = format!(
            "{},symbol={},underlying={} price={},basis={},settlement={},oi={},volume={} {}",
            self.config.stock_tables.futures,
            symbol,
            data.underlying,
            data.price,
            data.basis,
            data.settlement_price,
            data.open_interest,
            data.volume,
            data.timestamp * 1_000_000
        );
        
        self.add_to_buffer(line).await
    }
    */

    async fn add_to_buffer(&self, line: String) -> Result<()> {
        let mut buffer = self.buffer.write().await;
        buffer.push(line);

        // Check if we should flush
        if buffer.len() >= self.config.batch_size {
            drop(buffer);  // Release lock before flushing
            self.flush_internal().await?;
        } else {
            // Check time-based flush
            let last_flush = *self.last_flush.read().await;
            if (Utc::now() - last_flush).num_milliseconds() as u64 >= self.config.flush_interval_ms {
                drop(buffer);  // Release lock before flushing
                self.flush_internal().await?;
            }
        }

        Ok(())
    }

    /// Write a raw ILP (InfluxDB Line Protocol) line to QuestDB
    pub async fn write_ilp_line(&self, line: &str) -> Result<()> {
        self.add_to_buffer(line.to_string()).await
    }

    pub async fn flush(&self) -> Result<()> {
        self.flush_internal().await
    }

    async fn flush_internal(&self) -> Result<()> {
        let mut buffer = self.buffer.write().await;
        if buffer.is_empty() {
            return Ok(());
        }
        
        let data = buffer.join("\n");
        let num_lines = buffer.len();
        buffer.clear();
        
        info!("Flushing {} lines to QuestDB, {} bytes total", num_lines, data.len());
        debug!("ILP data being sent:\n{}", data);
        
        // Send to QuestDB via ILP protocol
        match self.socket.send(data.as_bytes()).await {
            Ok(bytes_sent) => {
                info!("Successfully sent {} bytes to QuestDB (UDP {})", bytes_sent, self.server_addr);
                *self.last_flush.write().await = Utc::now();
                Ok(())
            }
            Err(e) => {
                error!("Failed to send data to QuestDB: {}", e);
                Err(Error::Connection(format!("QuestDB send failed: {}", e)))
            }
        }
    }

    pub async fn create_tables(&self) -> Result<()> {
        // QuestDB automatically creates tables when data is first written
        // But we can pre-create them with optimal schema using HTTP API
        
        let client = reqwest::Client::new();
        let base_url = format!("http://{}:{}/exec", self.config.host, self.config.http_port);
        
        // Market data tables are handled by separate repos
        // TODO: Remove these once migration is complete
        /*
        // Create crypto tables
        let crypto_trades_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                exchange SYMBOL,
                symbol SYMBOL,
                price DOUBLE,
                quantity DOUBLE,
                value DOUBLE,
                timestamp TIMESTAMP
            ) timestamp(timestamp) PARTITION BY DAY;",
            self.config.crypto_tables.trades
        );
        */
        
        /*
        let crypto_orderbook_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                exchange SYMBOL,
                symbol SYMBOL,
                bid DOUBLE,
                ask DOUBLE,
                bid_size DOUBLE,
                ask_size DOUBLE,
                spread DOUBLE,
                timestamp TIMESTAMP
            ) timestamp(timestamp) PARTITION BY DAY;",
            self.config.crypto_tables.orderbook
        );
        */
        
        /*
        // Create stock tables
        let stock_trades_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                exchange SYMBOL,
                symbol SYMBOL,
                asset_type SYMBOL,
                price DOUBLE,
                quantity DOUBLE,
                value DOUBLE,
                timestamp TIMESTAMP
            ) timestamp(timestamp) PARTITION BY DAY;",
            self.config.stock_tables.trades
        );
        */
        
        /*
        let stock_options_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                symbol SYMBOL,
                underlying SYMBOL,
                type SYMBOL,
                strike DOUBLE,
                price DOUBLE,
                iv DOUBLE,
                delta DOUBLE,
                gamma DOUBLE,
                theta DOUBLE,
                vega DOUBLE,
                oi LONG,
                volume LONG,
                timestamp TIMESTAMP
            ) timestamp(timestamp) PARTITION BY DAY;",
            self.config.stock_tables.options
        );
        */
        
        // TODO: Execute feeder table creation
        /*
        for sql in &[connection_logs_sql, performance_logs_sql, error_logs_sql, processing_stats_sql] {
            let response = client.get(&base_url)
                .query(&[("query", sql)])
                .send()
                .await?;
                
            if !response.status().is_success() {
                warn!("Failed to create table: {}", response.text().await?);
            }
        }
        */
        
        info!("QuestDB tables initialized");
        Ok(())
    }

    pub async fn query(&self, sql: &str) -> std::result::Result<String, reqwest::Error> {
        let client = reqwest::Client::new();
        let base_url = format!("http://{}:{}/exec", self.config.host, self.config.http_port);
        
        let response = client.get(&base_url)
            .query(&[("query", sql)])
            .send()
            .await?;
            
        response.text().await
    }
}

#[async_trait]
impl DatabaseWriter for QuestDBClient {
    async fn write_trade(&mut self, data: &[u8]) -> Result<()> {
        // TODO: Implement feeder log methods
        // Deserialize and write trade data
        // if let Ok(trade) = serde_json::from_slice::<TradeData>(data) {
        //     // Determine if crypto or stock based on exchange or asset_type
        //     if trade.exchange == "binance" || trade.exchange == "bybit" || trade.exchange == "upbit" {
        //         self.write_crypto_trade(&trade).await?;
        //     } else {
        //         self.write_stock_trade(&trade).await?;
        //     }
        // }
        Ok(())
    }

    async fn write_orderbook(&mut self, data: &[u8]) -> Result<()> {
        // TODO: Implement feeder log methods
        // if let Ok(orderbook) = serde_json::from_slice::<OrderBookData>(data) {
        //     if orderbook.exchange == "binance" || orderbook.exchange == "bybit" || orderbook.exchange == "upbit" {
        //         self.write_crypto_orderbook(&orderbook).await?;
        //     } else {
        //         self.write_stock_orderbook(&orderbook).await?;
        //     }
        // }
        Ok(())
    }

    async fn write_metadata(&mut self, _data: &[u8]) -> Result<()> {
        // Metadata storage not implemented yet
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        self.flush_internal().await
    }

    async fn health_check(&self) -> Result<bool> {
        // Try to send a test line
        let test_line = format!("health_check,test=true value=1 {}", 
            Utc::now().timestamp_nanos_opt().unwrap_or(0));
        
        match self.socket.send(test_line.as_bytes()).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}