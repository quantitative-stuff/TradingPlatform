use crate::core::{RiskManager, OrderStatusChecker, OrderRequest};
use crate::error::{Result, Error};
use std::sync::Arc;
use async_trait::async_trait;

pub struct BinanceRiskManager {
    status_checker: Arc<dyn OrderStatusChecker>,
    max_orders_per_window: usize,
    order_window_ms: u64,
    last_orders_timestamp: std::sync::Mutex<i64>,
    order_count: std::sync::Mutex<usize>,
}

#[async_trait]
impl RiskManager for BinanceRiskManager {
    async fn validate_order(&self, order: &OrderRequest) -> Result<()> {
        // Check if we have enough balance
        let price = order.price.unwrap_or(0.0);  // For market orders we might need current price
        let available_orders = self.status_checker
            .calculate_max_orders(&order.symbol, price, order.quantity)
            .await?;

        if available_orders == 0 {
            return Err(Error::InvalidInput("Insufficient balance for order".into()));
        }

        // Check order rate limits
        let now = chrono::Utc::now().timestamp_millis();
        let mut last_timestamp = self.last_orders_timestamp.lock().unwrap();
        let mut count = self.order_count.lock().unwrap();

        if now - *last_timestamp > self.order_window_ms as i64 {
            // Reset counter for new window
            *count = 0;
            *last_timestamp = now;
        }

        if *count >= self.max_orders_per_window {
            return Err(Error::InvalidInput("Order rate limit exceeded".into()));
        }

        *count += 1;
        Ok(())
    }

    async fn adjust_order(&self, order: OrderRequest) -> Result<OrderRequest> {
        // Here we could implement logic to adjust order size based on available balance
        // For now we just validate
        self.validate_order(&order).await?;
        Ok(order)
    }
}