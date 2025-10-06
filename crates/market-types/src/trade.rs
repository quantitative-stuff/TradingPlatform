use serde::{Deserialize, Serialize};
use crate::{Exchange, Timestamp, Price, Quantity, Symbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub timestamp: Timestamp,
    pub price: Price,
    pub quantity: Quantity,
    pub side: Side,
    pub trade_id: String,
}

impl Trade {
    pub fn new(
        exchange: Exchange,
        symbol: Symbol,
        timestamp: Timestamp,
        price: Price,
        quantity: Quantity,
        side: Side,
        trade_id: String,
    ) -> Self {
        Self {
            exchange,
            symbol,
            timestamp,
            price,
            quantity,
            side,
            trade_id,
        }
    }
}
