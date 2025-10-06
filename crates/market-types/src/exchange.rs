use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Binance,
    BinanceFutures,
    Bybit,
    BybitLinear,
    Upbit,
    Coinbase,
    OKX,
    Deribit,
    Bithumb,
    LSExchange,
}

impl Exchange {
    pub fn as_str(&self) -> &'static str {
        match self {
            Exchange::Binance => "binance",
            Exchange::BinanceFutures => "binance-futures",
            Exchange::Bybit => "bybit",
            Exchange::BybitLinear => "bybit-linear",
            Exchange::Upbit => "upbit",
            Exchange::Coinbase => "coinbase",
            Exchange::OKX => "okx",
            Exchange::Deribit => "deribit",
            Exchange::Bithumb => "bithumb",
            Exchange::LSExchange => "ls-exchange",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "binance" => Some(Exchange::Binance),
            "binance-futures" => Some(Exchange::BinanceFutures),
            "bybit" => Some(Exchange::Bybit),
            "bybit-linear" => Some(Exchange::BybitLinear),
            "upbit" => Some(Exchange::Upbit),
            "coinbase" => Some(Exchange::Coinbase),
            "okx" => Some(Exchange::OKX),
            "deribit" => Some(Exchange::Deribit),
            "bithumb" => Some(Exchange::Bithumb),
            "ls-exchange" | "ls" => Some(Exchange::LSExchange),
            _ => None,
        }
    }
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
