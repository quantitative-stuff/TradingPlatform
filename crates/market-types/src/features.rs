use serde::{Deserialize, Serialize};
use crate::{Exchange, Timestamp};

/// Market features calculated from order book and trades
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketFeatures {
    pub timestamp: Timestamp,
    pub symbol: String,
    pub exchange: Exchange,

    // Price features
    pub wap_0ms: Option<f64>,
    pub wap_100ms: Option<f64>,
    pub wap_200ms: Option<f64>,
    pub wap_300ms: Option<f64>,

    // Order book features
    pub order_flow_imbalance: Option<f64>,
    pub order_book_imbalance: Option<f64>,
    pub liquidity_imbalance: Option<f64>,
    pub queue_imbalance: Option<f64>,

    // Trade flow features
    pub tick_flow_imbalance: Option<f64>,
    pub trade_flow_imbalance: Option<f64>,

    // Spread features
    pub spread: Option<f64>,
    pub spread_bps: Option<f64>,

    // Volume features
    pub bid_volume_10bps: Option<f64>,
    pub ask_volume_10bps: Option<f64>,
    pub bid_volume_20bps: Option<f64>,
    pub ask_volume_20bps: Option<f64>,
}

/// NBBO (National Best Bid and Offer) across exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NBBO {
    pub symbol: String,
    pub timestamp: Timestamp,
    pub best_bid: Option<(f64, f64, Exchange)>, // (price, quantity, exchange)
    pub best_ask: Option<(f64, f64, Exchange)>, // (price, quantity, exchange)
}
