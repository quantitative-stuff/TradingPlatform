//! Matching Engine - L2 to L3 order book reconstruction
//!
//! This module converts Level 2 (aggregated) order book updates
//! into Level 3 (individual order) events for analysis.

pub mod engine;
pub mod types;
pub mod orderbook_builder;
pub mod checksum;
pub mod persistence;
pub mod windows_opt;
pub mod monitoring;
pub mod hft_orderbook;

pub use engine::MatchingEngine;
pub use types::*;

// Re-export market types for convenience
pub use market_types::{
    Exchange, Side, Trade, OrderBook, OrderBookUpdate, PriceLevel,
    OrderEvent, MarketFeatures, Signal, SignalType, NBBO, Timestamp
};
