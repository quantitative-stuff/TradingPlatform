use serde::{Deserialize, Serialize};
use crate::{Exchange, Timestamp};
use crate::features::MarketFeatures;

/// Trading signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub timestamp: Timestamp,
    pub symbol: String,
    pub exchange: Exchange,
    pub signal_type: SignalType,
    pub strength: f64, // -1.0 to 1.0
    pub confidence: f64, // 0.0 to 1.0
    pub features: MarketFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalType {
    MeanReversion,
    Momentum,
    MarketMaking,
    Arbitrage,
}
