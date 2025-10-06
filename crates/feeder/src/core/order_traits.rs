use async_trait::async_trait;
use crate::error::Result;
use std::sync::Arc;
use crate::core::order_types::{OrderRequest, OrderResponse, Position, OrderSide, Balance};
use crate::core::types::MarketData;

#[async_trait]
pub trait OrderExecutor: Send + Sync {
    async fn send_order(&self, order: OrderRequest) -> Result<OrderResponse>;
    async fn cancel_order(&self, order_id: &str) -> Result<OrderResponse>;
    async fn cancel_all_orders(&self, symbol: &str) -> Result<Vec<OrderResponse>>;
    async fn place_limit_order(&self, symbol: &str, side: OrderSide, quantity: f64, price: f64) -> Result<OrderResponse>;
    async fn place_market_order(&self, symbol: &str, side: OrderSide, quantity: f64) -> Result<OrderResponse>;
    async fn send_multiple_orders(&self, orders: Vec<OrderRequest>) -> Result<Vec<OrderResponse>>;

}

#[async_trait]
pub trait OrderStatusChecker: Send + Sync {
    async fn query_order_status(&self, exchange: &str, symbol: &str, order_id: &str) -> Result<OrderResponse>;
    async fn get_open_orders(&self, symbol: &str) -> Result<Vec<OrderResponse>>;
    async fn get_positions(&self, symbol: &str) -> Result<Vec<Position>>;
    async fn close_position(&self, position: &Position, executor: &Arc<dyn OrderExecutor>) -> Result<OrderResponse>;
    async fn get_account_balance(&self) -> Result<Vec<Balance>>;
    async fn calculate_max_orders(&self, symbol: &str, price: f64, size: f64) -> Result<usize>;
}

/// The RiskManager trait is used for pre-trade validations and order adjustments.
/// Implement this to enforce risk policies before submitting orders.
#[async_trait]
pub trait RiskManager: Send + Sync {
    async fn validate_order(&self, order: &OrderRequest) -> Result<()>;
    async fn adjust_order(&self, order: OrderRequest) -> Result<OrderRequest>;
}

/// The OrderNotificationHandler trait is used to receive asynchronous notifications
/// about order updates (e.g., fills, rejections, modifications).
#[async_trait]
pub trait OrderNotificationHandler: Send + Sync {
    async fn on_order_update(&self, order_response: OrderResponse);
}

/// The Strategy trait encapsulates trading logic in response to market data.
/// Implementations can generate order requests based on incoming market data.
#[async_trait]
pub trait Strategy: Send + Sync {
    async fn on_market_data(&self, data: MarketData);
    async fn generate_order(&self, data: MarketData) -> Option<OrderRequest>;
}