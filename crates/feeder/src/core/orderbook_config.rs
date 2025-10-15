/// OrderBook Builder configuration
///
/// Configuration for selective orderbook building to avoid heavy load

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookBuilderConfig {
    /// Enable orderbook building for this exchange
    #[serde(default)]
    pub enabled: bool,

    /// Build orderbooks only for these symbols (empty = all symbols)
    #[serde(default)]
    pub symbols: Vec<String>,

    /// Maximum number of orderbook builders (to limit resource usage)
    #[serde(default = "default_max_builders")]
    pub max_builders: usize,

    /// Only build orderbooks for top N most active symbols
    #[serde(default)]
    pub top_n_symbols: Option<usize>,

    /// Buffer size for each builder
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    /// Enable persistence (saves snapshots to disk)
    #[serde(default)]
    pub enable_persistence: bool,

    /// Send built orderbooks via UDP
    #[serde(default)]
    pub send_via_udp: bool,
}

fn default_max_builders() -> usize {
    10  // Limit to 10 builders by default
}

fn default_buffer_size() -> usize {
    500  // Smaller buffer to reduce memory
}

impl Default for OrderBookBuilderConfig {
    fn default() -> Self {
        Self {
            enabled: false,  // Disabled by default
            symbols: Vec::new(),
            max_builders: default_max_builders(),
            top_n_symbols: None,
            buffer_size: default_buffer_size(),
            enable_persistence: false,
            send_via_udp: false,
        }
    }
}

impl OrderBookBuilderConfig {
    /// Check if orderbook building should be enabled for a symbol
    pub fn should_build(&self, symbol: &str) -> bool {
        if !self.enabled {
            return false;
        }

        // If symbols list is empty, build for all
        if self.symbols.is_empty() {
            return true;
        }

        // Otherwise, only build for specified symbols
        self.symbols.iter().any(|s| s == symbol)
    }

    /// Get symbols to build as a HashSet for faster lookup
    pub fn get_symbol_set(&self) -> HashSet<String> {
        self.symbols.iter().cloned().collect()
    }
}