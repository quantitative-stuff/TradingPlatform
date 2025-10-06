use serde::{Deserialize, Serialize};
use std::fs::File;
use anyhow::Result;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeederConfig {
    pub feeder: FeederSettings,
    pub exchange_profiles: ExchangeProfiles,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeederSettings {
    pub enabled_exchanges: Vec<String>,
    pub udp_mode: String,
    pub udp_settings: UdpSettings,
    pub logging: LoggingSettings,
    pub development: DevelopmentSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UdpSettings {
    pub buffer_size: usize,
    pub flush_interval_ms: u64,
    pub multicast_address: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingSettings {
    pub level: String,
    pub file_logging: bool,
    pub console_logging: bool,
    pub questdb_logging: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DevelopmentSettings {
    pub limited_mode: bool,
    pub terminal_display: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExchangeProfiles {
    pub crypto_major: Vec<String>,
    pub crypto_asian: Vec<String>,
    pub crypto_derivatives: Vec<String>,
    pub crypto_all: Vec<String>,
    pub single_binance: Vec<String>,
    pub single_bybit: Vec<String>,
}

impl FeederConfig {
    pub fn load(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let config: FeederConfig = serde_json::from_reader(file)?;
        Ok(config)
    }

    pub fn load_default() -> Result<Self> {
        Self::load("config/shared/feeder_config.json")
    }

    pub fn get_enabled_exchanges(&self) -> &Vec<String> {
        &self.feeder.enabled_exchanges
    }

    pub fn set_exchange_profile(&mut self, profile_name: &str) -> Result<()> {
        let exchanges = match profile_name {
            "crypto_major" => &self.exchange_profiles.crypto_major,
            "crypto_asian" => &self.exchange_profiles.crypto_asian,
            "crypto_derivatives" => &self.exchange_profiles.crypto_derivatives,
            "crypto_all" => &self.exchange_profiles.crypto_all,
            "single_binance" => &self.exchange_profiles.single_binance,
            "single_bybit" => &self.exchange_profiles.single_bybit,
            _ => return Err(anyhow::anyhow!("Unknown exchange profile: {}", profile_name)),
        };

        self.feeder.enabled_exchanges = exchanges.clone();
        Ok(())
    }

    pub fn is_direct_mode(&self) -> bool {
        self.feeder.udp_mode == "direct"
    }

    pub fn is_buffered_mode(&self) -> bool {
        self.feeder.udp_mode == "buffered"
    }

    pub fn get_available_exchanges() -> Vec<&'static str> {
        vec!["binance", "bybit", "upbit", "coinbase", "okx", "deribit", "bithumb"]
    }

    pub fn validate(&self) -> Result<()> {
        let available_exchanges = Self::get_available_exchanges();

        for exchange in &self.feeder.enabled_exchanges {
            if !available_exchanges.contains(&exchange.as_str()) {
                return Err(anyhow::anyhow!("Unknown exchange: {}", exchange));
            }
        }

        if self.feeder.enabled_exchanges.is_empty() {
            return Err(anyhow::anyhow!("No exchanges enabled"));
        }

        if !["direct", "buffered"].contains(&self.feeder.udp_mode.as_str()) {
            return Err(anyhow::anyhow!("UDP mode must be 'direct' or 'buffered'"));
        }

        Ok(())
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}

// Command line interface for config management
pub fn print_config_info(config: &FeederConfig) {
    println!("📋 Feeder Configuration:");
    println!("  🏢 Enabled Exchanges: {:?}", config.feeder.enabled_exchanges);
    println!("  📡 UDP Mode: {}", config.feeder.udp_mode);
    println!("  📦 Buffer Size: {}", config.feeder.udp_settings.buffer_size);
    println!("  ⏱️  Flush Interval: {}ms", config.feeder.udp_settings.flush_interval_ms);
    println!("  📝 Logging: file={}, console={}, questdb={}",
             config.feeder.logging.file_logging,
             config.feeder.logging.console_logging,
             config.feeder.logging.questdb_logging);
    println!("  🔧 Development: limited={}, terminal={}",
             config.feeder.development.limited_mode,
             config.feeder.development.terminal_display);
}

pub fn print_available_profiles(config: &FeederConfig) {
    println!("📚 Available Exchange Profiles:");
    println!("  crypto_major: {:?}", config.exchange_profiles.crypto_major);
    println!("  crypto_asian: {:?}", config.exchange_profiles.crypto_asian);
    println!("  crypto_derivatives: {:?}", config.exchange_profiles.crypto_derivatives);
    println!("  crypto_all: {:?}", config.exchange_profiles.crypto_all);
    println!("  single_binance: {:?}", config.exchange_profiles.single_binance);
    println!("  single_bybit: {:?}", config.exchange_profiles.single_bybit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_loading() {
        let config = FeederConfig::load_default();
        assert!(config.is_ok());
    }

    #[test]
    fn test_exchange_validation() {
        let mut config = FeederConfig::load_default().unwrap();
        config.feeder.enabled_exchanges = vec!["invalid_exchange".to_string()];
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_profile_switching() {
        let mut config = FeederConfig::load_default().unwrap();
        config.set_exchange_profile("crypto_all").unwrap();
        assert_eq!(config.feeder.enabled_exchanges.len(), 7);
    }
}