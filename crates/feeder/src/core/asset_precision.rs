use std::collections::HashMap;
use std::sync::RwLock;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

/// Precision configuration for an asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPrecision {
    pub symbol: String,
    pub price_decimals: u8,      // Number of decimal places for price
    pub quantity_decimals: u8,   // Number of decimal places for quantity
    pub scale_factor: i64,       // 10^price_decimals for fast multiplication
    pub min_price: f64,          // Minimum price (e.g., 0.00000001)
    pub min_quantity: f64,       // Minimum quantity (e.g., 0.001)
}

impl AssetPrecision {
    pub fn new(symbol: &str, price_decimals: u8, quantity_decimals: u8) -> Self {
        let scale_factor = 10_i64.pow(price_decimals as u32);
        let min_price = 1.0 / scale_factor as f64;
        let min_quantity = 1.0 / 10_i64.pow(quantity_decimals as u32) as f64;
        
        Self {
            symbol: symbol.to_string(),
            price_decimals,
            quantity_decimals,
            scale_factor,
            min_price,
            min_quantity,
        }
    }
    
    /// Scale a price value to integer representation
    pub fn scale_price(&self, price: f64) -> i64 {
        (price * self.scale_factor as f64).round() as i64
    }
    
    /// Unscale an integer price back to float
    pub fn unscale_price(&self, scaled_price: i64) -> f64 {
        scaled_price as f64 / self.scale_factor as f64
    }
    
    /// Scale a quantity value to integer representation
    pub fn scale_quantity(&self, quantity: f64) -> i64 {
        let scale = 10_i64.pow(self.quantity_decimals as u32);
        (quantity * scale as f64).round() as i64
    }
}

/// Global precision manager
pub struct PrecisionManager {
    // Key format: "EXCHANGE:SYMBOL" e.g., "Binance:BTC/USDT"
    precisions: RwLock<HashMap<String, AssetPrecision>>,
    exchange_defaults: RwLock<HashMap<String, HashMap<String, u8>>>, // exchange -> asset_type -> decimals
}

impl PrecisionManager {
    pub fn new() -> Self {
        let mut precisions = HashMap::new();
        let mut exchange_defaults = HashMap::new();
        
        // Initialize with EXCHANGE-SPECIFIC precisions
        // Format: "Exchange:Symbol" -> AssetPrecision
        
        // Binance precisions
        precisions.insert("Binance:BTC/USDT".to_string(), AssetPrecision::new("BTC/USDT", 2, 6));  // Price: 2 decimals, Qty: 6
        precisions.insert("Binance:ETH/USDT".to_string(), AssetPrecision::new("ETH/USDT", 2, 5));
        precisions.insert("Binance:SHIB/USDT".to_string(), AssetPrecision::new("SHIB/USDT", 8, 0)); // SHIB: 8 price decimals, 0 qty
        precisions.insert("Binance:PEPE/USDT".to_string(), AssetPrecision::new("PEPE/USDT", 8, 0));
        
        // Upbit precisions (different from Binance!)
        precisions.insert("Upbit:BTC/KRW".to_string(), AssetPrecision::new("BTC/KRW", 0, 8));     // KRW: 0 decimals
        precisions.insert("Upbit:ETH/KRW".to_string(), AssetPrecision::new("ETH/KRW", 0, 8));
        precisions.insert("Upbit:BTC/USDT".to_string(), AssetPrecision::new("BTC/USDT", 4, 6));   // Different from Binance!
        precisions.insert("Upbit:SHIB/KRW".to_string(), AssetPrecision::new("SHIB/KRW", 2, 0));   // KRW allows 2 decimals for SHIB
        
        // Coinbase precisions
        precisions.insert("Coinbase:BTC/USD".to_string(), AssetPrecision::new("BTC/USD", 2, 8));  // USD: 2 decimals
        precisions.insert("Coinbase:ETH/USD".to_string(), AssetPrecision::new("ETH/USD", 2, 8));
        precisions.insert("Coinbase:SHIB/USD".to_string(), AssetPrecision::new("SHIB/USD", 8, 0));
        
        // Bybit precisions
        precisions.insert("Bybit:BTC/USDT".to_string(), AssetPrecision::new("BTC/USDT", 2, 6));
        precisions.insert("Bybit:SHIB/USDT".to_string(), AssetPrecision::new("SHIB/USDT", 9, 0)); // Bybit allows 9 decimals!
        
        // OKX precisions
        precisions.insert("OKX:BTC/USDT".to_string(), AssetPrecision::new("BTC/USDT", 1, 8));    // OKX: 1 decimal for price
        precisions.insert("OKX:SHIB/USDT".to_string(), AssetPrecision::new("SHIB/USDT", 10, 0));  // OKX: 10 decimals for SHIB
        
        // Stock/futures (typically 2 decimals)
        precisions.insert("AAPL".to_string(), AssetPrecision::new("AAPL", 2, 0));
        precisions.insert("TSLA".to_string(), AssetPrecision::new("TSLA", 2, 0));
        precisions.insert("SPY".to_string(), AssetPrecision::new("SPY", 2, 0));
        
        // Exchange-specific defaults
        let mut binance_defaults = HashMap::new();
        binance_defaults.insert("spot".to_string(), 8);
        binance_defaults.insert("futures".to_string(), 8);
        exchange_defaults.insert("Binance".to_string(), binance_defaults);
        
        let mut upbit_defaults = HashMap::new();
        upbit_defaults.insert("KRW".to_string(), 0);  // KRW pairs have 0 decimals
        upbit_defaults.insert("USDT".to_string(), 8);
        exchange_defaults.insert("Upbit".to_string(), upbit_defaults);
        
        let mut coinbase_defaults = HashMap::new();
        coinbase_defaults.insert("USD".to_string(), 2);
        coinbase_defaults.insert("USDT".to_string(), 8);
        exchange_defaults.insert("Coinbase".to_string(), coinbase_defaults);
        
        Self {
            precisions: RwLock::new(precisions),
            exchange_defaults: RwLock::new(exchange_defaults),
        }
    }
    
    /// Get precision for a symbol, with fallback to defaults
    pub fn get_precision(&self, symbol: &str, exchange: Option<&str>) -> AssetPrecision {
        // First, try exchange-specific precision
        if let Some(exchange_name) = exchange {
            let key = format!("{}:{}", exchange_name, symbol);
            if let Some(precision) = self.precisions.read().unwrap().get(&key) {
                return precision.clone();
            }
            
            // Try exchange defaults
            if let Some(defaults) = self.exchange_defaults.read().unwrap().get(exchange_name) {
                // Parse quote currency from symbol (e.g., "BTC/USDT" -> "USDT")
                if let Some(quote) = symbol.split('/').nth(1) {
                    if let Some(&decimals) = defaults.get(quote) {
                        return AssetPrecision::new(symbol, decimals, 8);
                    }
                }
            }
        }
        
        // Fallback: try symbol without exchange (for backward compatibility)
        if let Some(precision) = self.precisions.read().unwrap().get(symbol) {
            return precision.clone();
        }
        
        // Default fallback: 8 decimals (safe for most crypto)
        AssetPrecision::new(symbol, 8, 8)
    }
    
    /// Update precision from exchange info
    pub fn update_from_exchange(&self, exchange: &str, symbol: &str, price_decimals: u8, quantity_decimals: u8) {
        let key = format!("{}:{}", exchange, symbol);
        let precision = AssetPrecision::new(symbol, price_decimals, quantity_decimals);
        self.precisions.write().unwrap().insert(key.clone(), precision);
        
        // Log the update for monitoring
        tracing::info!(
            "Updated precision for {} on {}: price={} decimals, qty={} decimals",
            symbol, exchange, price_decimals, quantity_decimals
        );
    }
    
    /// Load precision config from file
    pub async fn load_from_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = tokio::fs::read_to_string(path).await?;
        let precisions: HashMap<String, AssetPrecision> = serde_json::from_str(&content)?;
        
        let mut current = self.precisions.write().unwrap();
        current.extend(precisions);
        
        Ok(())
    }
    
    /// Save current precision config to file
    pub async fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let precisions = self.precisions.read().unwrap().clone();
        let json = serde_json::to_string_pretty(&precisions)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }
    
    /// Get scale factor for fast lookup
    pub fn get_scale_factor(&self, symbol: &str, exchange: Option<&str>) -> i64 {
        self.get_precision(symbol, exchange).scale_factor
    }
}

// Global precision manager instance
pub static PRECISION_MANAGER: Lazy<PrecisionManager> = Lazy::new(PrecisionManager::new);

/// Helper functions for easy access
pub fn scale_price(symbol: &str, price: f64, exchange: Option<&str>) -> i64 {
    let precision = PRECISION_MANAGER.get_precision(symbol, exchange);
    precision.scale_price(price)
}

pub fn unscale_price(symbol: &str, scaled_price: i64, exchange: Option<&str>) -> f64 {
    let precision = PRECISION_MANAGER.get_precision(symbol, exchange);
    precision.unscale_price(scaled_price)
}

pub fn get_price_decimals(symbol: &str, exchange: Option<&str>) -> u8 {
    PRECISION_MANAGER.get_precision(symbol, exchange).price_decimals
}

/// Update precision from exchange data (called when receiving exchange info)
pub fn update_precision_from_exchange(exchange: &str, symbol: &str, price_decimals: u8, quantity_decimals: u8) {
    PRECISION_MANAGER.update_from_exchange(exchange, symbol, price_decimals, quantity_decimals);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_btc_precision() {
        let btc_price = 50000.12345678;
        let scaled = scale_price("BTC/USDT", btc_price, Some("Binance"));
        assert_eq!(scaled, 5000012345678);
        
        let unscaled = unscale_price("BTC/USDT", scaled, Some("Binance"));
        assert!((unscaled - btc_price).abs() < 0.00000001);
    }
    
    #[test]
    fn test_shib_precision() {
        let shib_price = 0.000000012345;
        let scaled = scale_price("SHIB/USDT", shib_price, Some("Binance"));
        // With 12 decimals, this should preserve the precision
        let unscaled = unscale_price("SHIB/USDT", scaled, Some("Binance"));
        assert!((unscaled - shib_price).abs() < 0.000000000001);
    }
    
    #[test]
    fn test_krw_precision() {
        let btc_krw_price = 65000000.0; // 65 million KRW
        let scaled = scale_price("BTC/KRW", btc_krw_price, Some("Upbit"));
        assert_eq!(scaled, 65000000); // No decimals for KRW
        
        let unscaled = unscale_price("BTC/KRW", scaled, Some("Upbit"));
        assert_eq!(unscaled, btc_krw_price);
    }
}