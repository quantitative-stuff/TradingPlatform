/// Fast JSON parsing using SIMD
/// 5-10x faster than serde_json for hot paths
/// Based on: docs/feeder/udp_packet_instruction.md

use simd_json;
use anyhow::{Result, anyhow};

/// Fast parser for common exchange message patterns
pub struct FastJsonParser {
    // Pre-allocated buffer for mutable parsing
    buffer: Vec<u8>,
}

impl FastJsonParser {
    /// Create new parser with buffer capacity
    pub fn new(capacity: usize) -> Self {
        FastJsonParser {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Parse JSON bytes into simd_json value (mutable, fast)
    pub fn parse_mut(&mut self, data: &[u8]) -> Result<simd_json::OwnedValue> {
        // Clear and copy data to mutable buffer
        self.buffer.clear();
        self.buffer.extend_from_slice(data);

        // Parse with SIMD acceleration
        simd_json::to_owned_value(&mut self.buffer)
            .map_err(|e| anyhow!("SIMD JSON parse error: {}", e))
    }

    /// Parse JSON string
    pub fn parse_str(&mut self, data: &str) -> Result<simd_json::OwnedValue> {
        self.parse_mut(data.as_bytes())
    }
}

/// Extract f64 from JSON value (common in exchange feeds)
pub fn get_f64(value: &simd_json::OwnedValue, key: &str) -> Option<f64> {
    match value {
        simd_json::OwnedValue::Object(obj) => {
            match obj.get(key)? {
                simd_json::OwnedValue::Static(simd_json::StaticNode::F64(v)) => Some(*v),
                simd_json::OwnedValue::Static(simd_json::StaticNode::I64(v)) => Some(*v as f64),
                simd_json::OwnedValue::Static(simd_json::StaticNode::U64(v)) => Some(*v as f64),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract i64 from JSON value
pub fn get_i64(value: &simd_json::OwnedValue, key: &str) -> Option<i64> {
    match value {
        simd_json::OwnedValue::Object(obj) => {
            match obj.get(key)? {
                simd_json::OwnedValue::Static(simd_json::StaticNode::I64(v)) => Some(*v),
                simd_json::OwnedValue::Static(simd_json::StaticNode::U64(v)) => Some(*v as i64),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract u64 from JSON value (common for timestamps)
pub fn get_u64(value: &simd_json::OwnedValue, key: &str) -> Option<u64> {
    match value {
        simd_json::OwnedValue::Object(obj) => {
            match obj.get(key)? {
                simd_json::OwnedValue::Static(simd_json::StaticNode::U64(v)) => Some(*v),
                simd_json::OwnedValue::Static(simd_json::StaticNode::I64(v)) => Some(*v as u64),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract string from JSON value
pub fn get_str<'a>(value: &'a simd_json::OwnedValue, key: &str) -> Option<&'a str> {
    match value {
        simd_json::OwnedValue::Object(obj) => {
            match obj.get(key)? {
                simd_json::OwnedValue::String(s) => Some(s.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract bool from JSON value
pub fn get_bool(value: &simd_json::OwnedValue, key: &str) -> Option<bool> {
    match value {
        simd_json::OwnedValue::Object(obj) => {
            match obj.get(key)? {
                simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(v)) => Some(*v),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract array from JSON value
pub fn get_array<'a>(value: &'a simd_json::OwnedValue, key: &str) -> Option<&'a Vec<simd_json::OwnedValue>> {
    match value {
        simd_json::OwnedValue::Object(obj) => {
            match obj.get(key)? {
                simd_json::OwnedValue::Array(arr) => Some(arr),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse Binance trade message (optimized hot path)
#[derive(Debug, Clone)]
pub struct BinanceTrade {
    pub symbol: String,
    pub price: f64,
    pub quantity: f64,
    pub timestamp: u64,
    pub is_buyer_maker: bool,
}

impl BinanceTrade {
    /// Parse from simd_json value
    pub fn from_json(value: &simd_json::OwnedValue) -> Option<Self> {
        Some(BinanceTrade {
            symbol: get_str(value, "s")?.to_string(),
            price: get_str(value, "p")?.parse().ok()?,
            quantity: get_str(value, "q")?.parse().ok()?,
            timestamp: get_u64(value, "T")?,
            is_buyer_maker: get_bool(value, "m")?,
        })
    }
}

/// Parse Binance orderbook message (optimized hot path)
#[derive(Debug, Clone)]
pub struct BinanceOrderBook {
    pub symbol: String,
    pub bids: Vec<(f64, f64)>,
    pub asks: Vec<(f64, f64)>,
    pub timestamp: u64,
}

impl BinanceOrderBook {
    /// Parse from simd_json value
    pub fn from_json(value: &simd_json::OwnedValue) -> Option<Self> {
        let symbol = get_str(value, "s")?.to_string();
        let timestamp = get_u64(value, "E")?;

        let bids = parse_price_level_array(get_array(value, "b")?)?;
        let asks = parse_price_level_array(get_array(value, "a")?)?;

        Some(BinanceOrderBook {
            symbol,
            bids,
            asks,
            timestamp,
        })
    }
}

/// Parse array of [price, quantity] arrays
fn parse_price_level_array(arr: &[simd_json::OwnedValue]) -> Option<Vec<(f64, f64)>> {
    arr.iter()
        .map(|level| {
            let level_arr = match level {
                simd_json::OwnedValue::Array(a) => a,
                _ => return None,
            };
            if level_arr.len() < 2 {
                return None;
            }
            let price_str = match &level_arr[0] {
                simd_json::OwnedValue::String(s) => s.as_str(),
                _ => return None,
            };
            let qty_str = match &level_arr[1] {
                simd_json::OwnedValue::String(s) => s.as_str(),
                _ => return None,
            };
            let price = price_str.parse().ok()?;
            let qty = qty_str.parse().ok()?;
            Some((price, qty))
        })
        .collect()
}

/// Benchmark helper: compare serde_json vs simd_json
#[cfg(test)]
pub fn benchmark_parsing(json_str: &str, iterations: usize) {
    use std::time::Instant;

    // serde_json baseline
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::from_str::<serde_json::Value>(json_str);
    }
    let serde_duration = start.elapsed();

    // simd_json
    let mut parser = FastJsonParser::new(json_str.len() + 128);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = parser.parse_str(json_str);
    }
    let simd_duration = start.elapsed();

    println!("=== JSON Parsing Benchmark ({} iterations) ===", iterations);
    println!("serde_json: {:?} ({:.2} ns/iter)", serde_duration, serde_duration.as_nanos() as f64 / iterations as f64);
    println!("simd_json:  {:?} ({:.2} ns/iter)", simd_duration, simd_duration.as_nanos() as f64 / iterations as f64);
    println!("Speedup: {:.2}x", serde_duration.as_nanos() as f64 / simd_duration.as_nanos() as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_parser_basic() {
        let mut parser = FastJsonParser::new(256);
        let json = r#"{"price": 50000.5, "qty": 1.23, "ts": 1234567890}"#;

        let value = parser.parse_str(json).unwrap();

        assert_eq!(get_f64(&value, "price"), Some(50000.5));
        assert_eq!(get_f64(&value, "qty"), Some(1.23));
        assert_eq!(get_u64(&value, "ts"), Some(1234567890));
    }

    #[test]
    fn test_binance_trade_parsing() {
        let mut parser = FastJsonParser::new(512);
        let json = r#"{
            "e": "trade",
            "E": 1234567890,
            "s": "BTCUSDT",
            "t": 12345,
            "p": "50000.00",
            "q": "0.001",
            "T": 1234567890,
            "m": true
        }"#;

        let value = parser.parse_str(json).unwrap();
        let trade = BinanceTrade::from_json(&value).unwrap();

        assert_eq!(trade.symbol, "BTCUSDT");
        assert_eq!(trade.price, 50000.0);
        assert_eq!(trade.quantity, 0.001);
        assert_eq!(trade.timestamp, 1234567890);
        assert_eq!(trade.is_buyer_maker, true);
    }

    #[test]
    fn test_binance_orderbook_parsing() {
        let mut parser = FastJsonParser::new(1024);
        let json = r#"{
            "e": "depthUpdate",
            "E": 1234567890,
            "s": "BTCUSDT",
            "b": [
                ["50000.00", "1.5"],
                ["49999.00", "2.0"]
            ],
            "a": [
                ["50001.00", "1.2"],
                ["50002.00", "1.8"]
            ]
        }"#;

        let value = parser.parse_str(json).unwrap();
        let book = BinanceOrderBook::from_json(&value).unwrap();

        assert_eq!(book.symbol, "BTCUSDT");
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.asks.len(), 2);
        assert_eq!(book.bids[0], (50000.0, 1.5));
        assert_eq!(book.asks[0], (50001.0, 1.2));
    }

    #[test]
    fn test_benchmark() {
        let json = r#"{
            "e": "trade",
            "E": 1234567890,
            "s": "BTCUSDT",
            "t": 12345,
            "p": "50000.00",
            "q": "0.001",
            "T": 1234567890,
            "m": true,
            "extra": "data",
            "nested": {"key": "value"}
        }"#;

        benchmark_parsing(json, 10000);
    }
}