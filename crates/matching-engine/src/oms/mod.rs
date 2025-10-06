use crate::types::{
    Order, OrderStatus, OrderType, Position, Side, TimeInForce,
    Timestamp, Exchange, PerformanceMetrics
};
use anyhow::{Result, bail};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct OrderManagementSystem {
    // Order tracking
    orders: Arc<DashMap<u64, Order>>,
    order_id_counter: Arc<AtomicU64>,

    // Position tracking
    positions: Arc<DashMap<(String, Exchange), Arc<RwLock<Position>>>>,

    // Risk limits
    max_position_size: f64,
    max_daily_loss: f64,
    max_total_exposure: f64,

    // Performance tracking
    performance: Arc<RwLock<PerformanceMetrics>>,
    daily_pnl: Arc<RwLock<f64>>,
}

impl OrderManagementSystem {
    pub fn new(
        max_position_size: f64,
        max_daily_loss: f64,
        max_total_exposure: f64,
    ) -> Self {
        Self {
            orders: Arc::new(DashMap::new()),
            order_id_counter: Arc::new(AtomicU64::new(1)),
            positions: Arc::new(DashMap::new()),
            max_position_size,
            max_daily_loss,
            max_total_exposure,
            performance: Arc::new(RwLock::new(PerformanceMetrics {
                total_trades: 0,
                winning_trades: 0,
                losing_trades: 0,
                total_pnl: 0.0,
                sharpe_ratio: 0.0,
                max_drawdown: 0.0,
                win_rate: 0.0,
                avg_win: 0.0,
                avg_loss: 0.0,
            })),
            daily_pnl: Arc::new(RwLock::new(0.0)),
        }
    }

    /// Submit a new order
    pub fn submit_order(
        &self,
        symbol: String,
        exchange: Exchange,
        side: Side,
        order_type: OrderType,
        quantity: f64,
        price: Option<f64>,
        timestamp: Timestamp,
    ) -> Result<u64> {
        // Pre-trade risk checks
        self.check_risk_limits(&symbol, exchange, side, quantity)?;

        // Generate order ID
        let order_id = self.order_id_counter.fetch_add(1, Ordering::SeqCst);

        // Create order
        let order = Order {
            id: order_id,
            timestamp,
            symbol: symbol.clone(),
            exchange,
            side,
            order_type,
            quantity,
            price,
            time_in_force: TimeInForce::IOC,
            status: OrderStatus::New,
        };

        // Store order
        self.orders.insert(order_id, order);

        Ok(order_id)
    }

    /// Update order status
    pub fn update_order_status(
        &self,
        order_id: u64,
        new_status: OrderStatus,
        timestamp: Timestamp,
    ) -> Result<()> {
        let mut order = self.orders
            .get_mut(&order_id)
            .ok_or_else(|| anyhow::anyhow!("Order not found"))?;

        order.status = new_status.clone();

        // Update position if filled
        if let OrderStatus::Filled { filled_quantity, avg_price } = &new_status {
            self.update_position(
                &order.symbol,
                order.exchange,
                order.side,
                *filled_quantity,
                *avg_price,
                timestamp,
            )?;
        }

        Ok(())
    }

    /// Cancel an order
    pub fn cancel_order(&self, order_id: u64) -> Result<()> {
        let mut order = self.orders
            .get_mut(&order_id)
            .ok_or_else(|| anyhow::anyhow!("Order not found"))?;

        match &order.status {
            OrderStatus::New | OrderStatus::Pending => {
                order.status = OrderStatus::Cancelled;
                Ok(())
            }
            _ => bail!("Cannot cancel order in current state"),
        }
    }

    /// Update position after trade execution
    fn update_position(
        &self,
        symbol: &str,
        exchange: Exchange,
        side: Side,
        quantity: f64,
        price: f64,
        timestamp: Timestamp,
    ) -> Result<()> {
        let key = (symbol.to_string(), exchange);
        let position = self.positions
            .entry(key.clone())
            .or_insert_with(|| Arc::new(RwLock::new(Position {
                symbol: symbol.to_string(),
                exchange,
                quantity: 0.0,
                avg_entry_price: 0.0,
                unrealized_pnl: 0.0,
                realized_pnl: 0.0,
                last_update: timestamp,
            })))
            .clone();

        let mut pos = position.write();

        // Calculate new position
        let signed_quantity = match side {
            Side::Buy => quantity,
            Side::Sell => -quantity,
        };

        let old_quantity = pos.quantity;
        let new_quantity = old_quantity + signed_quantity;

        // Check if closing or reducing position
        if old_quantity != 0.0 && old_quantity.signum() != new_quantity.signum() {
            // Position is being closed or reversed
            let closed_quantity = if new_quantity.signum() != old_quantity.signum() {
                old_quantity.abs()
            } else {
                quantity
            };

            // Calculate realized PnL
            let pnl = closed_quantity * (price - pos.avg_entry_price) * old_quantity.signum();
            pos.realized_pnl += pnl;

            // Update daily PnL
            *self.daily_pnl.write() += pnl;

            // Update performance metrics
            self.update_performance_metrics(pnl);
        }

        // Update position
        if new_quantity != 0.0 {
            if old_quantity.signum() == new_quantity.signum() {
                // Adding to position
                let total_cost = pos.avg_entry_price * old_quantity.abs() + price * quantity;
                pos.avg_entry_price = total_cost / new_quantity.abs();
            } else {
                // New position after reversal
                pos.avg_entry_price = price;
            }
            pos.quantity = new_quantity;
        } else {
            // Position closed
            pos.quantity = 0.0;
            pos.avg_entry_price = 0.0;
        }

        pos.last_update = timestamp;

        Ok(())
    }

    /// Check risk limits before placing order
    fn check_risk_limits(
        &self,
        symbol: &str,
        exchange: Exchange,
        side: Side,
        quantity: f64,
    ) -> Result<()> {
        // Check daily loss limit
        let daily_pnl = *self.daily_pnl.read();
        if daily_pnl < -self.max_daily_loss {
            bail!("Daily loss limit exceeded");
        }

        // Check position size limit
        let key = (symbol.to_string(), exchange);
        if let Some(position) = self.positions.get(&key) {
            let pos = position.read();
            let new_quantity = match side {
                Side::Buy => pos.quantity + quantity,
                Side::Sell => pos.quantity - quantity,
            };
            if new_quantity.abs() > self.max_position_size {
                bail!("Position size limit exceeded");
            }
        } else if quantity > self.max_position_size {
            bail!("Position size limit exceeded");
        }

        // Check total exposure
        let total_exposure = self.calculate_total_exposure();
        if total_exposure > self.max_total_exposure {
            bail!("Total exposure limit exceeded");
        }

        Ok(())
    }

    /// Calculate total exposure across all positions
    fn calculate_total_exposure(&self) -> f64 {
        self.positions
            .iter()
            .map(|entry| {
                let pos = entry.value().read();
                pos.quantity.abs() * pos.avg_entry_price
            })
            .sum()
    }

    /// Update performance metrics
    fn update_performance_metrics(&self, pnl: f64) {
        let mut metrics = self.performance.write();

        metrics.total_trades += 1;
        metrics.total_pnl += pnl;

        if pnl > 0.0 {
            metrics.winning_trades += 1;
        } else if pnl < 0.0 {
            metrics.losing_trades += 1;
        }

        // Update win rate
        if metrics.total_trades > 0 {
            metrics.win_rate = metrics.winning_trades as f64 / metrics.total_trades as f64;
        }

        // Update average win/loss
        if metrics.winning_trades > 0 {
            metrics.avg_win = metrics.total_pnl.max(0.0) / metrics.winning_trades as f64;
        }
        if metrics.losing_trades > 0 {
            metrics.avg_loss = metrics.total_pnl.min(0.0) / metrics.losing_trades as f64;
        }

        // Update max drawdown (simplified)
        if metrics.total_pnl < metrics.max_drawdown {
            metrics.max_drawdown = metrics.total_pnl;
        }
    }

    /// Get current position for a symbol
    pub fn get_position(&self, symbol: &str, exchange: Exchange) -> Option<Position> {
        let key = (symbol.to_string(), exchange);
        self.positions
            .get(&key)
            .map(|pos_ref| pos_ref.read().clone())
    }

    /// Get all open positions
    pub fn get_all_positions(&self) -> Vec<Position> {
        self.positions
            .iter()
            .map(|entry| entry.value().read().clone())
            .filter(|pos| pos.quantity != 0.0)
            .collect()
    }

    /// Get performance metrics
    pub fn get_performance(&self) -> PerformanceMetrics {
        self.performance.read().clone()
    }

    /// Reset daily PnL (call at start of each day)
    pub fn reset_daily_pnl(&self) {
        *self.daily_pnl.write() = 0.0;
    }
}