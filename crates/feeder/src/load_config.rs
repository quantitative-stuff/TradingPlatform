use crate::error::{Result, Error};
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;

/// Timestamp unit used by the exchange
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TimestampUnit {
    Milliseconds,  // Most common: Binance, Bybit, OKX, etc.
    Microseconds,  // Some exchanges
    Nanoseconds,   // Rare but possible
    Seconds,       // Some legacy APIs
}

impl TimestampUnit {
    /// Convert timestamp to nanoseconds for internal storage
    pub fn to_nanoseconds(&self, timestamp: u64) -> u64 {
        // Add overflow detection to find which exchange has wrong config
        let result = match self {
            TimestampUnit::Seconds => timestamp.checked_mul(1_000_000_000),
            TimestampUnit::Milliseconds => timestamp.checked_mul(1_000_000),
            TimestampUnit::Microseconds => timestamp.checked_mul(1_000),
            TimestampUnit::Nanoseconds => Some(timestamp),
        };

        match result {
            Some(nanos) => nanos,
            None => {
                // Log the overflow with context so we can identify the exchange
                tracing::error!("⚠️  TIMESTAMP OVERFLOW DETECTED!");
                tracing::error!("   Input timestamp: {}", timestamp);
                tracing::error!("   Attempted conversion: {:?} -> Nanoseconds", self);
                tracing::error!("   This likely means the exchange config has wrong timestamp_unit");
                tracing::error!("   Timestamp looks like it's already in: {}", Self::detect_likely_unit(timestamp));
                panic!("Fix the exchange config file with correct timestamp_unit!");
            }
        }
    }

    /// Detect what unit a timestamp is likely in based on its magnitude
    fn detect_likely_unit(timestamp: u64) -> &'static str {
        if timestamp < 10_000_000_000 {
            "Seconds (10-digit Unix timestamp)"
        } else if timestamp < 10_000_000_000_000 {
            "Milliseconds (13-digit timestamp)"
        } else if timestamp < 10_000_000_000_000_000 {
            "Microseconds (16-digit timestamp)"
        } else {
            "Nanoseconds (19-digit timestamp)"
        }
    }

    /// Convert nanoseconds back to the exchange's native unit
    pub fn from_nanoseconds(&self, nanos: u64) -> u64 {
        match self {
            TimestampUnit::Seconds => nanos / 1_000_000_000,
            TimestampUnit::Milliseconds => nanos / 1_000_000,
            TimestampUnit::Microseconds => nanos / 1_000,
            TimestampUnit::Nanoseconds => nanos,
        }
    }
}

impl Default for TimestampUnit {
    fn default() -> Self {
        TimestampUnit::Milliseconds
    }
}

// Defines what Binance config looks like
#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeConfig {
    pub output_dir: PathBuf,
    pub feed_config: FeedConfig,
    pub subscribe_data: SubscribeData,
    pub connect_config: ConnectConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedConfig {
    pub exchange: String,
    pub asset_type: Vec<String>,
    #[serde(default)]
    pub timestamp_unit: TimestampUnit,
    #[serde(default = "default_price_precision")]
    pub price_precision: u8,      // Default number of decimals for price
    #[serde(default = "default_quantity_precision")]
    pub quantity_precision: u8,   // Default number of decimals for quantity
    #[serde(default)]
    pub symbol_precision: HashMap<String, SymbolPrecision>, // Per-symbol overrides
}

/// Per-symbol precision overrides
#[derive(Debug, Clone, Deserialize)]
pub struct SymbolPrecision {
    #[serde(default = "default_price_precision")]
    pub price_precision: u8,
    #[serde(default = "default_quantity_precision")]
    pub quantity_precision: u8,
}

fn default_price_precision() -> u8 { 8 }
fn default_quantity_precision() -> u8 { 8 }

impl FeedConfig {
    /// Get price precision for a specific symbol (uses override if exists)
    pub fn get_price_precision(&self, symbol: &str) -> u8 {
        self.symbol_precision
            .get(symbol)
            .map(|sp| sp.price_precision)
            .unwrap_or(self.price_precision)
    }

    /// Get quantity precision for a specific symbol (uses override if exists)
    pub fn get_quantity_precision(&self, symbol: &str) -> u8 {
        self.symbol_precision
            .get(symbol)
            .map(|sp| sp.quantity_precision)
            .unwrap_or(self.quantity_precision)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscribeData {
    #[serde(default)]
    pub codes: Vec<String>, // Legacy field for backward compatibility
    #[serde(default)]
    pub spot_symbols: Vec<String>,
    #[serde(default)]
    pub futures_symbols: Vec<String>,
    #[serde(default)]
    pub option_symbols: Vec<String>,
    pub stream_type: Vec<String>,
    #[serde(alias = "orderdepth", alias = "level", alias = "depth")]
    pub order_depth: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectConfig {
    pub ws_url: String,
    pub testnet_url: String,
    pub mainnet_url: String,
    pub use_testnet: bool,
    
    // Rate limit configuration
    #[serde(default = "default_connection_delay_ms")]
    pub connection_delay_ms: u64,  // Delay between creating connections
    
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,    // Maximum concurrent connections
    
    #[serde(default = "default_max_retry_delay_secs")]
    pub max_retry_delay_secs: u64, // Maximum retry delay after errors
    
    #[serde(default = "default_initial_retry_delay_secs")]
    pub initial_retry_delay_secs: u64, // Initial retry delay
}

// Default values for backwards compatibility
fn default_connection_delay_ms() -> u64 { 100 }
fn default_max_connections() -> usize { 100 }
fn default_max_retry_delay_secs() -> u64 { 300 }
fn default_initial_retry_delay_secs() -> u64 { 5 }

#[derive(Debug, Clone, Deserialize)]
pub struct TradingConfig {
    pub max_position: f64,
    pub min_spread: f64,
    pub tick_size: f64
}

impl ExchangeConfig {
    // Loads Binance-specific config from JSON
    pub fn new(file_name: &str) -> Result<Self> {
        let data = fs::read_to_string(file_name)?;
        let config: ExchangeConfig = serde_json::from_str(&data)?;
        Ok(config)
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = File::open(&path)
            .map_err(|e| Error::InvalidInput(format!("Failed to open config file: {}", e)))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| Error::InvalidInput(format!("Failed to read config file: {}", e)))?;

        // Determine file type by extension and parse accordingly
        let path_str = path.as_ref().to_string_lossy();

        if path_str.ends_with(".json") {
            serde_json::from_str(&contents)
                .map_err(|e| Error::InvalidInput(format!("Failed to parse JSON config: {}", e)))
        } else if path_str.ends_with(".toml") {
            toml::from_str(&contents)
                .map_err(|e| Error::InvalidInput(format!("Failed to parse TOML config: {}", e)))
        } else {
            Err(Error::InvalidInput("Unsupported config file format".into()))
        }
    }

}

/// Metadata for normalizing data from a specific exchange and symbol
/// Used to convert raw market data to normalized internal format
#[derive(Debug, Clone)]
pub struct SymbolMeta {
    pub exchange: String,
    pub symbol: String,
    pub exchange_id: u8,      // Numeric ID for the exchange
    pub symbol_id: u16,       // Numeric ID for the symbol
    pub price_scale: i64,     // e.g., 100_000_000 (1e8)
    pub qty_scale: i64,       // e.g., 100_000_000 (1e8)
    pub timestamp_unit: TimestampUnit,
}

impl SymbolMeta {
    pub fn new(
        exchange: String,
        symbol: String,
        exchange_id: u8,
        symbol_id: u16,
        timestamp_unit: TimestampUnit,
    ) -> Self {
        SymbolMeta {
            exchange,
            symbol,
            exchange_id,
            symbol_id,
            price_scale: 100_000_000,  // Default 1e8
            qty_scale: 100_000_000,    // Default 1e8
            timestamp_unit,
        }
    }
}

/// Global mapping table for exchange and symbol IDs
/// This allows efficient lookup during normalization
#[derive(Debug, Clone)]
pub struct MetadataRegistry {
    exchange_to_id: HashMap<String, u8>,
    id_to_exchange: HashMap<u8, String>,
    symbol_to_id: HashMap<(String, String), u16>, // (exchange, symbol) -> id
    id_to_symbol: HashMap<u16, (String, String)>,  // id -> (exchange, symbol)
    symbol_metadata: HashMap<(u8, u16), SymbolMeta>, // (exchange_id, symbol_id) -> meta
    next_exchange_id: u8,
    next_symbol_id: u16,
}

impl MetadataRegistry {
    pub fn new() -> Self {
        MetadataRegistry {
            exchange_to_id: HashMap::new(),
            id_to_exchange: HashMap::new(),
            symbol_to_id: HashMap::new(),
            id_to_symbol: HashMap::new(),
            symbol_metadata: HashMap::new(),
            next_exchange_id: 1,
            next_symbol_id: 1,
        }
    }

    /// Register or get exchange ID
    pub fn register_exchange(&mut self, exchange: &str) -> u8 {
        if let Some(&id) = self.exchange_to_id.get(exchange) {
            return id;
        }
        let id = self.next_exchange_id;
        self.exchange_to_id.insert(exchange.to_string(), id);
        self.id_to_exchange.insert(id, exchange.to_string());
        self.next_exchange_id += 1;
        id
    }

    /// Register or get symbol ID
    pub fn register_symbol(&mut self, exchange: &str, symbol: &str) -> u16 {
        let key = (exchange.to_string(), symbol.to_string());
        if let Some(&id) = self.symbol_to_id.get(&key) {
            return id;
        }
        let id = self.next_symbol_id;
        self.symbol_to_id.insert(key.clone(), id);
        self.id_to_symbol.insert(id, key);
        self.next_symbol_id += 1;
        id
    }

    /// Register symbol metadata
    pub fn register_metadata(&mut self, meta: SymbolMeta) {
        let key = (meta.exchange_id, meta.symbol_id);
        self.symbol_metadata.insert(key, meta);
    }

    /// Get exchange ID by name
    pub fn get_exchange_id(&self, exchange: &str) -> Option<u8> {
        self.exchange_to_id.get(exchange).copied()
    }

    /// Get symbol ID by exchange and symbol name
    pub fn get_symbol_id(&self, exchange: &str, symbol: &str) -> Option<u16> {
        let key = (exchange.to_string(), symbol.to_string());
        self.symbol_to_id.get(&key).copied()
    }

    /// Get metadata for a symbol
    pub fn get_metadata(&self, exchange_id: u8, symbol_id: u16) -> Option<&SymbolMeta> {
        self.symbol_metadata.get(&(exchange_id, symbol_id))
    }

    /// Get exchange name by ID
    pub fn get_exchange_name(&self, exchange_id: u8) -> Option<&String> {
        self.id_to_exchange.get(&exchange_id)
    }

    /// Get symbol name by ID
    pub fn get_symbol_name(&self, symbol_id: u16) -> Option<&(String, String)> {
        self.id_to_symbol.get(&symbol_id)
    }
}
