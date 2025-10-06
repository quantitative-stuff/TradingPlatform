/// Symbol mapping for cache indexing
/// Maps exchange and symbol names to integer IDs for fast array access

use std::collections::HashMap;
use std::sync::RwLock;
use once_cell::sync::Lazy;

/// Exchange ID mapping
pub static EXCHANGE_MAP: Lazy<RwLock<SymbolMapper>> = Lazy::new(|| {
    let mapper = SymbolMapper::new_with_defaults();
    RwLock::new(mapper)
});

/// Symbol name mapper for fast integer-based indexing
pub struct SymbolMapper {
    exchange_to_id: HashMap<String, usize>,
    id_to_exchange: Vec<String>,
    symbol_to_id: HashMap<String, usize>,
    id_to_symbol: Vec<String>,
}

impl SymbolMapper {
    /// Create new empty mapper
    pub fn new() -> Self {
        SymbolMapper {
            exchange_to_id: HashMap::new(),
            id_to_exchange: Vec::new(),
            symbol_to_id: HashMap::new(),
            id_to_symbol: Vec::new(),
        }
    }

    /// Create mapper with default exchange IDs
    pub fn new_with_defaults() -> Self {
        let mut mapper = Self::new();

        // Crypto exchanges
        mapper.add_exchange("binance");
        mapper.add_exchange("bybit");
        mapper.add_exchange("upbit");
        mapper.add_exchange("coinbase");
        mapper.add_exchange("okx");
        mapper.add_exchange("deribit");
        mapper.add_exchange("bithumb");

        // Stock exchanges
        mapper.add_exchange("ls");

        mapper
    }

    /// Add exchange if not exists, return ID
    pub fn add_exchange(&mut self, name: &str) -> usize {
        let normalized = name.to_lowercase();

        if let Some(&id) = self.exchange_to_id.get(&normalized) {
            return id;
        }

        let id = self.id_to_exchange.len();
        self.exchange_to_id.insert(normalized.clone(), id);
        self.id_to_exchange.push(normalized);
        id
    }

    /// Get exchange ID from name
    pub fn get_exchange_id(&self, name: &str) -> Option<usize> {
        self.exchange_to_id.get(&name.to_lowercase()).copied()
    }

    /// Get exchange name from ID
    pub fn get_exchange_name(&self, id: usize) -> Option<&str> {
        self.id_to_exchange.get(id).map(|s| s.as_str())
    }

    /// Add symbol if not exists, return ID
    pub fn add_symbol(&mut self, name: &str) -> usize {
        let normalized = name.to_uppercase();

        if let Some(&id) = self.symbol_to_id.get(&normalized) {
            return id;
        }

        let id = self.id_to_symbol.len();
        self.symbol_to_id.insert(normalized.clone(), id);
        self.id_to_symbol.push(normalized);
        id
    }

    /// Get symbol ID from name
    pub fn get_symbol_id(&self, name: &str) -> Option<usize> {
        self.symbol_to_id.get(&name.to_uppercase()).copied()
    }

    /// Get symbol name from ID
    pub fn get_symbol_name(&self, id: usize) -> Option<&str> {
        self.id_to_symbol.get(id).map(|s| s.as_str())
    }

    /// Get total number of exchanges
    pub fn num_exchanges(&self) -> usize {
        self.id_to_exchange.len()
    }

    /// Get total number of symbols
    pub fn num_symbols(&self) -> usize {
        self.id_to_symbol.len()
    }

    /// Parse null-terminated bytes to string
    pub fn parse_name(bytes: &[u8]) -> String {
        let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..null_pos]).to_string()
    }
}

/// Global helper functions
pub fn map_exchange(name: &str) -> Option<usize> {
    EXCHANGE_MAP.read().ok()?.get_exchange_id(name)
}

pub fn map_symbol(name: &str) -> Option<usize> {
    EXCHANGE_MAP.write().ok()?.get_symbol_id(name)
        .or_else(|| {
            let mut mapper = EXCHANGE_MAP.write().ok()?;
            Some(mapper.add_symbol(name))
        })
}

pub fn get_exchange_name(id: usize) -> Option<String> {
    EXCHANGE_MAP.read().ok()?.get_exchange_name(id).map(|s| s.to_string())
}

pub fn get_symbol_name(id: usize) -> Option<String> {
    EXCHANGE_MAP.read().ok()?.get_symbol_name(id).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_mapping() {
        let mut mapper = SymbolMapper::new();

        let binance_id = mapper.add_exchange("binance");
        let bybit_id = mapper.add_exchange("bybit");

        assert_eq!(binance_id, 0);
        assert_eq!(bybit_id, 1);

        assert_eq!(mapper.get_exchange_id("binance"), Some(0));
        assert_eq!(mapper.get_exchange_id("BINANCE"), Some(0)); // Case insensitive
        assert_eq!(mapper.get_exchange_name(0), Some("binance"));
    }

    #[test]
    fn test_symbol_mapping() {
        let mut mapper = SymbolMapper::new();

        let btc_id = mapper.add_symbol("BTC/USDT");
        let eth_id = mapper.add_symbol("ETH/USDT");

        assert_eq!(btc_id, 0);
        assert_eq!(eth_id, 1);

        assert_eq!(mapper.get_symbol_id("BTC/USDT"), Some(0));
        assert_eq!(mapper.get_symbol_id("btc/usdt"), Some(0)); // Case insensitive
        assert_eq!(mapper.get_symbol_name(0), Some("BTC/USDT"));
    }

    #[test]
    fn test_parse_name() {
        let bytes = b"binance\0\0\0\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(SymbolMapper::parse_name(bytes), "binance");

        let bytes = b"BTC/USDT\0\0\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(SymbolMapper::parse_name(bytes), "BTC/USDT");
    }

    #[test]
    fn test_defaults() {
        let mapper = SymbolMapper::new_with_defaults();

        assert_eq!(mapper.get_exchange_id("binance"), Some(0));
        assert_eq!(mapper.get_exchange_id("bybit"), Some(1));
        assert_eq!(mapper.get_exchange_id("upbit"), Some(2));
        assert_eq!(mapper.num_exchanges(), 8);
    }
}