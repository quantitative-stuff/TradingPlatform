use crate::error::{Result, Error};
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
