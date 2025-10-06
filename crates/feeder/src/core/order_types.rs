use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Debug, Clone)]
pub struct Balance {
    pub asset: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: f64,
    pub price: Option<f64>,  // Needed for limit orders, for example.
    pub timeinforce: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: String,
    pub status: String,
    pub price: Option<f64>,
    // Additional fields: e.g., filled quantity, cumulative fill, etc.
    pub filled_quantity: Option<f64>,
}


#[derive(Debug, Clone)]
pub struct Position {
    pub symbol: String,
    pub amount: f64,
    pub entry_price: f64,
    pub unrealized_profit: f64,
    pub side: String,  // "LONG" or "SHORT"
}