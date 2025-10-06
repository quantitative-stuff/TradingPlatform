use crate::error::Result;
use crate::secure_keys::SecureKeyLoader;

use serde::Deserialize;

// Defines what Binance config looks like
#[derive(Debug, Clone, Deserialize)]
pub struct LSConfig {
    pub app_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitConfig {
    pub exchange: String,
    pub asset_type: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscribeData {
    pub codes: Vec<String>,
    pub stream_type: Vec<String>,
    pub level: u32,
}

impl LSConfig {
    pub fn new(app_key: String, secret_key: String) -> Result<Self> {
        Ok(Self {
            app_key,
            secret_key,
        })
    }

    /// Load configuration with API keys from a secure key file
    pub fn from_secure_file(key_file_path: &str) -> Result<Self> {
        let loader = SecureKeyLoader::new(key_file_path.to_string());
        let (app_key, secret_key) = loader.load_keys()?;
        
        Ok(Self {
            app_key,
            secret_key,
        })
    }
}