use std::sync::RwLock;
use once_cell::sync::Lazy;
use crate::core::order_types::{OrderRequest, OrderResponse};
use crate::error::{Result, Error};

/// Represents an order with its exchange information
#[derive(Debug, Clone)]
pub struct TrackedOrder {
    pub order_id: String,
    pub exchange_id: String,
    pub symbol: String,
    pub original_request: OrderRequest,
    pub latest_response: OrderResponse,
}

/// Central manager for tracking orders across multiple exchanges
pub struct OrderManager {
    // Simple Vec for all orders
    orders: RwLock<Vec<TrackedOrder>>,
}

// Create a global instance using lazy_static
pub static ORDER_MANAGER: Lazy<OrderManager> = Lazy::new(|| OrderManager::new());


impl OrderManager {
    pub fn new() -> Self {
        Self {
            orders: RwLock::new(Vec::new()),
        }
    }

    /// Track a new order
    pub fn track_order(&self, exchange_id: &str, request: &OrderRequest, response: &OrderResponse) -> Result<()> {
        let tracked_order = TrackedOrder {
            order_id: response.order_id.clone(),
            exchange_id: exchange_id.to_string(),
            symbol: request.symbol.clone(),
            original_request: request.clone(),
            latest_response: response.clone(),
        };
        
        // Add to orders vec
        let mut orders = self.orders.write().unwrap();
        orders.push(tracked_order);
        
        Ok(())
    }

    /// Update an existing order with new status
    pub fn update_order(&self, order_id: &str, response: &OrderResponse) -> Result<()> {
        let mut orders = self.orders.write().unwrap();
        
        if let Some(tracked_order) = orders.iter_mut().find(|order| order.order_id == order_id) {
            tracked_order.latest_response = response.clone();
            Ok(())
        } else {
            Err(Error::InvalidInput(format!("Order not found: {}", order_id)))
        }
    }

    /// Get all order IDs across all exchanges
    pub fn get_all_order_ids(&self) -> Vec<String> {
        let orders = self.orders.read().unwrap();
        orders.iter().map(|order| order.order_id.clone()).collect()
    }

    /// Get order IDs for a specific exchange
    pub fn get_exchange_order_ids(&self, exchange_id: &str) -> Vec<String> {
        let orders = self.orders.read().unwrap();
        orders
            .iter()
            .filter(|order| order.exchange_id == exchange_id)
            .map(|order| order.order_id.clone())
            .collect()
    }

    /// Get order IDs for a specific symbol across all exchanges
    pub fn get_symbol_order_ids(&self, symbol: &str) -> Vec<String> {
        let orders = self.orders.read().unwrap();
        orders
            .iter()
            .filter(|order| order.symbol == symbol)
            .map(|order| order.order_id.clone())
            .collect()
    }

    /// Get order IDs for a specific symbol on a specific exchange
    pub fn get_exchange_symbol_order_ids(&self, exchange_id: &str, symbol: &str) -> Vec<String> {
        let orders = self.orders.read().unwrap();
        orders
            .iter()
            .filter(|order| order.exchange_id == exchange_id && order.symbol == symbol)
            .map(|order| order.order_id.clone())
            .collect()
    }

    /// Get details for a specific order
    pub fn get_order_details(&self, order_id: &str) -> Option<TrackedOrder> {
        let orders = self.orders.read().unwrap();
        orders
            .iter()
            .find(|order| order.order_id == order_id)
            .cloned()
    }
    
    /// Get all tracked orders
    pub fn get_all_orders(&self) -> Vec<TrackedOrder> {
        let orders = self.orders.read().unwrap();
        orders.clone()
    }
    
    /// Get all tracked orders for a specific exchange
    pub fn get_exchange_orders(&self, exchange_id: &str) -> Vec<TrackedOrder> {
        let orders = self.orders.read().unwrap();
        orders
            .iter()
            .filter(|order| order.exchange_id == exchange_id)
            .cloned()
            .collect()
    }
}