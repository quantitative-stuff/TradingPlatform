use serde::{Deserialize, Serialize};
use market_types::{Exchange, Side, Timestamp};

/// Order for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: u64,
    pub timestamp: Timestamp,
    pub symbol: String,
    pub exchange: Exchange,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: f64,
    pub price: Option<f64>, // For limit orders
    pub time_in_force: TimeInForce,
    pub status: OrderStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    IOC, // Immediate or Cancel
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimeInForce {
    GTC, // Good Till Cancel
    IOC, // Immediate or Cancel
    FOK, // Fill or Kill
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    Pending,
    PartiallyFilled { filled_quantity: f64 },
    Filled { filled_quantity: f64, avg_price: f64 },
    Cancelled,
    Rejected { reason: String },
}

/// Position tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub exchange: Exchange,
    pub quantity: f64, // positive for long, negative for short
    pub avg_entry_price: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub last_update: Timestamp,
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_trades: u64,
    pub winning_trades: u64,
    pub losing_trades: u64,
    pub total_pnl: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
}
