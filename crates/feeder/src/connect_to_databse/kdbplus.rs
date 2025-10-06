use crate::error::{Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::{TcpStream, SocketAddr};
use std::io::{Write, Read};
use chrono::{DateTime, Utc};
use tracing::{info, debug};

// kdb+ configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdbConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
}

impl Default for KdbConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5001,
            username: None,
            password: None,
            batch_size: 1000,
            flush_interval_ms: 1000,
        }
    }
}

// kdb+ IPC protocol implementation
pub struct KdbClient {
    config: KdbConfig,
    connection: Option<TcpStream>,
    buffer: Vec<TradeData>,
    orderbook_buffer: Vec<OrderBookData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub timestamp: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub price: f64,
    pub quantity: f64,
    pub side: String,
    pub trade_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookData {
    pub timestamp: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub bids: Vec<(f64, f64)>,  // (price, quantity)
    pub asks: Vec<(f64, f64)>,
}

impl KdbClient {
    pub async fn new(config: KdbConfig) -> Result<Self> {
        let mut client = Self {
            config: config.clone(),
            connection: None,
            buffer: Vec::with_capacity(config.batch_size),
            orderbook_buffer: Vec::with_capacity(config.batch_size),
        };
        
        client.connect().await?;
        Ok(client)
    }
    
    async fn connect(&mut self) -> Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .map_err(|e| Error::Config(format!("Invalid kdb+ address: {}", e)))?;
        
        info!("Connecting to kdb+ at {}", addr);
        
        let stream = TcpStream::connect(addr)
            .map_err(|e| Error::Connection(format!("Failed to connect to kdb+: {}", e)))?;
        
        // Send authentication if provided
        if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
            self.authenticate(&stream, user, pass)?;
        }
        
        self.connection = Some(stream);
        info!("Connected to kdb+ successfully");
        Ok(())
    }
    
    fn authenticate(&self, mut stream: &TcpStream, username: &str, password: &str) -> Result<()> {
        let auth_string = format!("{}:{}", username, password);
        let auth_bytes = auth_string.as_bytes();
        
        // kdb+ authentication protocol
        stream.write_all(&[0x01, 0x00])  // Protocol version
            .map_err(|e| Error::Connection(format!("Failed to send auth header: {}", e)))?;
        
        stream.write_all(auth_bytes)
            .map_err(|e| Error::Connection(format!("Failed to send credentials: {}", e)))?;
        
        // Read response
        let mut response = [0u8; 1];
        stream.read_exact(&mut response)
            .map_err(|e| Error::Connection(format!("Failed to read auth response: {}", e)))?;
        
        if response[0] != 0x01 {
                        return Err(Error::Auth("kdb+ authentication failed".to_string()));
        }
        
        Ok(())
    }
    
    pub async fn insert_trade(&mut self, trade: TradeData) -> Result<()> {
        self.buffer.push(trade);
        
        if self.buffer.len() >= self.config.batch_size {
            self.flush_trades().await?;
        }
        
        Ok(())
    }
    
    pub async fn insert_orderbook(&mut self, orderbook: OrderBookData) -> Result<()> {
        self.orderbook_buffer.push(orderbook);
        
        if self.orderbook_buffer.len() >= self.config.batch_size {
            self.flush_orderbooks().await?;
        }
        
        Ok(())
    }
    
    async fn flush_trades(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        
        let query = self.build_trade_insert_query(&self.buffer);
        self.execute_query(&query).await?;
        
        debug!("Flushed {} trades to kdb+", self.buffer.len());
        self.buffer.clear();
        Ok(())
    }
    
    async fn flush_orderbooks(&mut self) -> Result<()> {
        if self.orderbook_buffer.is_empty() {
            return Ok(());
        }
        
        let query = self.build_orderbook_insert_query(&self.orderbook_buffer);
        self.execute_query(&query).await?;
        
        debug!("Flushed {} orderbooks to kdb+", self.orderbook_buffer.len());
        self.orderbook_buffer.clear();
        Ok(())
    }
    
    fn build_trade_insert_query(&self, trades: &[TradeData]) -> String {
        // Build kdb+ insert statement
        let mut query = String::from("`trades insert (");
        
        // Timestamps
        query.push_str(".z.N;");
        for trade in trades {
            query.push_str(&format!("{};", trade.timestamp.timestamp_nanos()));
        }
        
        // Exchange symbols
        query.push_str("`");
        for trade in trades {
            query.push_str(&format!("{}`", trade.exchange));
        }
        
        // Symbols
        query.push_str("`");
        for trade in trades {
            query.push_str(&format!("{}`", trade.symbol));
        }
        
        // Prices
        for trade in trades {
            query.push_str(&format!("{}f;", trade.price));
        }
        
        // Quantities
        for trade in trades {
            query.push_str(&format!("{}f;", trade.quantity));
        }
        
        query.push(')');
        query
    }
    
    fn build_orderbook_insert_query(&self, orderbooks: &[OrderBookData]) -> String {
        // Build kdb+ insert for orderbook data
        let mut query = String::from("`orderbooks insert (");
        
        // Build columns similar to trades
        for ob in orderbooks {
            query.push_str(&format!(
                "(.z.N;`{};`{};",
                ob.exchange,
                ob.symbol
            ));
            
            // Bids as nested list
            query.push_str("(");
            for (price, qty) in &ob.bids {
                query.push_str(&format!("({};{});", price, qty));
            }
            query.push_str(");");
            
            // Asks as nested list
            query.push_str("(");
            for (price, qty) in &ob.asks {
                query.push_str(&format!("({};{});", price, qty));
            }
            query.push_str("));");
        }
        
        query
    }
    
    async fn execute_query(&mut self, query: &str) -> Result<()> {
        if self.connection.is_none() {
            self.connect().await?;
        }
        
        if let Some(ref mut conn) = self.connection {
            // Send query to kdb+
            let query_bytes = query.as_bytes();
            let len = query_bytes.len() as u32;
            
            // kdb+ IPC protocol: [msg_type(1)][0(1)][0(1)][0(1)][len(4)][data]
            conn.write_all(&[0x01, 0x00, 0x00, 0x00])
                .map_err(|e| Error::Connection(format!("Failed to send header: {}", e)))?;
            
            conn.write_all(&len.to_le_bytes())
                .map_err(|e| Error::Connection(format!("Failed to send length: {}", e)))?;
            
            conn.write_all(query_bytes)
                .map_err(|e| Error::Connection(format!("Failed to send query: {}", e)))?;
            
            // Read response
            let mut response_header = [0u8; 8];
            conn.read_exact(&mut response_header)
                .map_err(|e| Error::Connection(format!("Failed to read response: {}", e)))?;
            
            // Check for errors (simplified)
            if response_header[0] == 0x80 {
                return Err(Error::Database("kdb+ query execution failed".to_string()));
            }
            
            Ok(())
        } else {
            Err(Error::Connection("No active kdb+ connection".to_string()))
        }
    }
    
    pub async fn create_tables(&mut self) -> Result<()> {
        // Create trades table
        let create_trades = r#"
            trades:([] 
                time:`timestamp$(); 
                exchange:`symbol$(); 
                symbol:`symbol$(); 
                price:`float$(); 
                quantity:`float$(); 
                side:`symbol$();
                trade_id:`symbol$()
            )
        "#;
        
        // Create orderbooks table
        let create_orderbooks = r#"
            orderbooks:([] 
                time:`timestamp$(); 
                exchange:`symbol$(); 
                symbol:`symbol$(); 
                bids:(); 
                asks:()
            )
        "#;
        
        self.execute_query(create_trades).await?;
        self.execute_query(create_orderbooks).await?;
        
        info!("Created kdb+ tables successfully");
        Ok(())
    }
    
    pub async fn query(&mut self, q: &str) -> Result<String> {
        self.execute_query(q).await?;
        
        // Read and parse response (simplified)
        if let Some(ref mut conn) = self.connection {
            let mut buffer = Vec::new();
            conn.read_to_end(&mut buffer)
                .map_err(|e| Error::Connection(format!("Failed to read query result: {}", e)))?;
            
            // Parse kdb+ response (would need proper deserialization)
            Ok(String::from_utf8_lossy(&buffer).to_string())
        } else {
            Err(Error::Connection("No active connection".to_string()))
        }
    }
    
    pub async fn health_check(&self) -> Result<bool> {
        // Simple health check
        if let Some(ref conn) = self.connection {
            // Check if connection is still alive
            Ok(conn.peer_addr().is_ok())
        } else {
            Ok(false)
        }
    }
}

#[async_trait]
impl super::DatabaseWriter for KdbClient {
    async fn write_trade(&mut self, data: &[u8]) -> Result<()> {
        // Parse data and insert
        if let Ok(trade_str) = std::str::from_utf8(data) {
            // Parse JSON or your format
            if let Ok(trade) = serde_json::from_str::<TradeData>(trade_str) {
                self.insert_trade(trade).await?;
            }
        }
        Ok(())
    }
    
    async fn write_orderbook(&mut self, data: &[u8]) -> Result<()> {
        if let Ok(ob_str) = std::str::from_utf8(data) {
            if let Ok(orderbook) = serde_json::from_str::<OrderBookData>(ob_str) {
                self.insert_orderbook(orderbook).await?;
            }
        }
        Ok(())
    }
    
    async fn write_metadata(&mut self, _data: &[u8]) -> Result<()> {
        // Implement metadata storage if needed
        Ok(())
    }
    
    async fn flush(&mut self) -> Result<()> {
        self.flush_trades().await?;
        self.flush_orderbooks().await?;
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool> {
        self.health_check().await
    }
}

// Helper function to initialize kdb+ client
pub async fn init_kdb_client(config: KdbConfig) -> Result<KdbClient> {
    let mut client = KdbClient::new(config).await?;
    client.create_tables().await?;
    Ok(client)
}