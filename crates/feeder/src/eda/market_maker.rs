use std::sync::Arc;
use tokio::time::Duration;
use crate::core::{OrderExecutor, OrderStatusChecker, OrderSide, OrderType, 
                OrderRequest, MarketDataProvider};
use crate::core::ORDER_MANAGER;
use crate::error::{Result, Error};

/// Market making strategy helper functions
pub struct MarketMaker {
    pub client: Arc<dyn OrderExecutor>,
    pub status_checker: Arc<dyn OrderStatusChecker>,
    pub market_data_provider: Arc<dyn MarketDataProvider>,
    pub symbol: String,
    pub order_size: f64,
    pub spread: f64,
    pub tick_size: f64,
}

impl MarketMaker {
    pub fn new(
        client: Arc<dyn OrderExecutor>,
        status_checker: Arc<dyn OrderStatusChecker>,
        market_data_provider: Arc<dyn MarketDataProvider>,
        symbol: String,
        order_size: f64,
        spread: f64,
        tick_size: f64
    ) -> Self {
        Self {
            client,
            status_checker,
            market_data_provider,
            symbol,
            order_size,
            spread,
            tick_size,
        }
    }
    
    /// Cancel all open orders for the symbol - directly uses the client's method
    pub async fn cancel_all_orders(&self, exchange: &str, symbol: &str) -> Result<()> {
        println!("Cancelling all open orders for {}", self.symbol);
        
        // Get open orders to check if there's anything to cancel
        let open_orders = self.status_checker.get_open_orders(&self.symbol).await?;
        
        if open_orders.is_empty() {
            println!("No open orders to cancel");
            return Ok(());
        }
        
        println!("Found {} open orders to cancel", open_orders.len());
        
        // Use the client's cancel_all_orders method directly
        let cancel_results = self.client.cancel_all_orders(&self.symbol).await?;
        
        // Update order manager with the results
        for response in &cancel_results {
            if let Some(_) = ORDER_MANAGER.get_order_details(&response.order_id) {
                ORDER_MANAGER.update_order(&response.order_id, response)?;
            }
        }
        
        println!("Cancelled {} orders", cancel_results.len());
        
        Ok(())
    }
    
    /// Close all positions for the symbol
    pub async fn close_all_positions(&self) -> Result<()> {
        println!("Checking for open positions...");
        
        // Get positions using the status checker
        let positions = self.status_checker.get_positions(&self.symbol).await?;
        
        if positions.is_empty() {
            println!("No open positions to close");
            return Ok(());
        }
        
        println!("Found {} positions to close", positions.len());
        
        // Close each position using market orders
        for position in &positions {
            // Determine the side to close the position
            let close_side = if position.side == "LONG" { OrderSide::Sell } else { OrderSide::Buy };
            
            // Use the client's place_market_order method if available, otherwise use send_order
            let close_request = OrderRequest {
                symbol: position.symbol.clone(),
                side: close_side,
                order_type: OrderType::Market,
                quantity: position.amount,
                price: None,
                timeinforce: "GTC",
            };
            
            println!("Closing position: {:?}", position);
            
            // Send the order using the client
            match self.client.send_order(close_request.clone()).await {
                Ok(response) => {
                    // println!("Position closed with order ID: {}", response.order_id);
                    ORDER_MANAGER.track_order("binance", &close_request, &response)?;
                },
                Err(e) => {
                    eprintln!("Error closing position: {:?}", e);
                }
            }
        }
        
        // Wait for positions to be closed
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        // Verify positions are closed using the status checker
        let remaining_positions = self.status_checker.get_positions(&self.symbol).await?;
        if !remaining_positions.is_empty() {
            println!("Warning: {} positions still open after closing", remaining_positions.len());
        } else {
            println!("All positions successfully closed");
        }
        
        Ok(())
    }
    
    /// Clean up all orders and positions
    pub async fn cleanup(&self) -> Result<()> {
        println!("Cleaning up orders and positions for {}", self.symbol);
        
        // Cancel all orders first
        self.cancel_all_orders("binance", &self.symbol).await?;
        
        // Then close all positions
        self.close_all_positions().await?;
        
        println!("Cleanup complete");
        
        Ok(())
    }
    
    pub async fn place_multiple_orders(&self, mid_price: f64, levels: usize) -> Result<()> {
        println!("Placing {} levels of orders around mid price {}", levels, mid_price);
        
        let mut orders = Vec::with_capacity(levels * 2);
        
        // Binance Futures BTCUSDT specific rules
        const PRICE_PRECISION: i32 = 1;  // 1 decimal for price
        const QUANTITY_PRECISION: i32 = 3;  // 3 decimals for quantity
        const MIN_NOTIONAL: f64 = 100.0;  // Minimum order value in USDT
        
        // Helper function to round with precision
        let round_with_precision = |value: f64, precision: i32| -> f64 {
            let scale = 10f64.powi(precision);
            (value * scale).trunc() / scale
        };
    
        // Round mid price to valid precision
        let mid_price = round_with_precision(mid_price, PRICE_PRECISION);
        
        // Create bid orders at different price levels
        for i in 1..=levels {
            let price = round_with_precision(
                mid_price - (i as f64 * self.spread), 
                PRICE_PRECISION
            );
            
            // Calculate minimum quantity needed for notional value
            let min_quantity = (MIN_NOTIONAL / price).ceil();
            let raw_quantity = self.order_size.max(min_quantity);
            
            // Round quantity to valid precision
            let quantity = round_with_precision(raw_quantity, QUANTITY_PRECISION);
            
            // Verify notional value after rounding
            let notional = quantity * price;
            if notional < MIN_NOTIONAL {
                println!("Skipping bid order: price={}, qty={}, notional={} (< min {})", 
                        price, quantity, notional, MIN_NOTIONAL);
                continue;
            }
    
            // println!("Creating bid: price={}, qty={}, notional={}", price, quantity, notional);
            
            orders.push(OrderRequest {
                symbol: self.symbol.clone(),
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                quantity,
                price: Some(price),
                timeinforce: "GTC",
            });
        }
        
        // Create ask orders at different price levels
        for i in 1..=levels {
            let price = round_with_precision(
                mid_price + (i as f64 * self.spread), 
                PRICE_PRECISION
            );
            
            // Calculate minimum quantity needed for notional value
            let min_quantity = (MIN_NOTIONAL / price).ceil();
            let raw_quantity = self.order_size.max(min_quantity);
            
            // Round quantity to valid precision
            let quantity = round_with_precision(raw_quantity, QUANTITY_PRECISION);
            
            // Verify notional value after rounding
            let notional = quantity * price;
            if notional < MIN_NOTIONAL {
                println!("Skipping ask order: price={}, qty={}, notional={} (< min {})", 
                        price, quantity, notional, MIN_NOTIONAL);
                continue;
            }
    
            // println!("Creating ask: price={}, qty={}, notional={}", price, quantity, notional);
            
            orders.push(OrderRequest {
                symbol: self.symbol.clone(),
                side: OrderSide::Sell,
                order_type: OrderType::Limit,
                quantity,
                price: Some(price),
                timeinforce: "GTC",
            });
        }
        
        if orders.is_empty() {
            return Err(Error::InvalidInput("No valid orders could be created - check price/quantity precision and minimum notional".into()));
        }
    
        // Send orders and track them
        let orders_clone = orders.clone();
        let responses = self.client.send_multiple_orders(orders).await?;
        
        for (i, response) in responses.iter().enumerate() {
            if i < orders_clone.len() {
                ORDER_MANAGER.track_order("binance", &orders_clone[i], response)?;
            }
        }
        
        Ok(())
    }

    /// Place bid and ask orders around the current market price
    pub async fn place_orders(&self, exchange: &str, symbol: &str, mid_price: f64) -> Result<()> {
        // Calculate new bid and ask prices
        let raw_bid_price = mid_price - self.spread / 2.0;
        let raw_ask_price = mid_price + self.spread / 2.0;

        // Adjust prices to the nearest tick size
        let bid_price = (raw_bid_price / self.tick_size).floor() * self.tick_size;
        let ask_price = (raw_ask_price / self.tick_size).ceil() * self.tick_size;

        // Ensure precision (optional, depending on your language)
        let bid_price = (bid_price * 10.0).round() / 10.0; // Rounds to 2 decimals
        let ask_price = (ask_price * 10.0).round() / 10.0;        

        // Calculate minimum quantity needed to meet notional value requirement
        let min_notional = 100.0; // Binance requires minimum notional of 100 USDT
        let min_quantity_bid = min_notional / bid_price;
        let min_quantity_ask = min_notional / ask_price;
        
        // Use the larger of the two to ensure both orders meet the requirement
        let min_quantity = min_quantity_bid.max(min_quantity_ask);
        
        // Ensure our order size meets the minimum
        let quantity = self.order_size.max(min_quantity);
        
        // Round to appropriate precision (usually 3 decimal places for BTC)
        let quantity = (quantity * 1000.0).ceil() / 1000.0;

        // println!("Placing new orders: Bid @ {:.1}, Ask @ {:.1}, Quantity: {:.3}", 
        //         bid_price, ask_price, quantity);
        // println!("Notional values: Bid = {:.2}, Ask = {:.2}", 
        //         bid_price * quantity, ask_price * quantity);

        // Create bid request
        let bid_request = OrderRequest {
            symbol: symbol.to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: quantity,
            price: Some(bid_price),
            timeinforce: "GTC",
        };
        
        // Create ask request
        let ask_request = OrderRequest {
            symbol: symbol.to_string(),
            side: OrderSide::Sell,
            order_type: OrderType::Limit,
            quantity: quantity,
            price: Some(ask_price),
            timeinforce: "GTC",
        };
        
        // Send bid order
        let bid_response = self.client.send_order(bid_request.clone()).await?;
        // println!("Bid order placed: {}", bid_response.order_id);
        ORDER_MANAGER.track_order("binance", &bid_request, &bid_response)?;
        // println!("tracked bid order");
        
        // Send ask order
        let ask_response = self.client.send_order(ask_request.clone()).await?;
        // println!("Ask order placed: {}", ask_response.order_id);
        ORDER_MANAGER.track_order("binance", &ask_request, &ask_response)?;
        // println!("tracked ask order");
        Ok(())
    }
    
    /// Check if any orders have been filled
    pub async fn check_order_status(&self, exchange: &str, symbol: &str) -> Result<()> {
        // Get all order IDs for this symbol
        let order_ids = ORDER_MANAGER.get_exchange_symbol_order_ids(exchange, symbol);
        
        for order_id in order_ids {
            // Use the status checker to query each order
            let status = self.status_checker.query_order_status(exchange, symbol, &order_id).await?;
            
            // Update the order in the manager
            ORDER_MANAGER.update_order(&order_id, &status)?;
            
            // Log if the order was filled
            // match status.price {
            //     Some(price) => println!("Order {} was filled at price {}", order_id, price),
            //     None => println!("Order {} was filled (price unknown)", order_id),
            // }
        }
        
        Ok(())
    }
 
    // Place balanced orders (equal number on both sides)
    pub async fn place_balanced_orders(&self, mid_price: f64, levels: usize) -> Result<()> {
        println!("Placing balanced orders around mid price {}", mid_price);
        
        // Calculate max orders based on available margin
        let max_orders = self.status_checker.calculate_max_orders(
            &self.symbol, mid_price, self.order_size).await?;
        
        // Determine how many levels we can afford (each level is 2 orders - buy and sell)
        let affordable_levels = max_orders / 2;
        let actual_levels = levels.min(affordable_levels);
        
        println!("Using {} levels (max affordable: {})", actual_levels, affordable_levels);
        
        // Create a vector to hold all orders
        let mut orders = Vec::with_capacity(actual_levels * 2);
        
        // Round mid price to tick size
        let mid_price = (mid_price / self.tick_size).round() * self.tick_size;
        
        // Calculate minimum quantity to meet notional value requirement
        let min_notional = 100.0; // Binance requires minimum notional of 100 USDT
        
        // Create bid orders at different price levels
        for i in 1..=actual_levels {
            let price = mid_price - (i as f64 * self.spread);
            let price = (price / self.tick_size).round() * self.tick_size;
            
            // Ensure quantity meets minimum notional value
            let min_quantity = min_notional / price;
            let quantity = self.order_size.max(min_quantity);
            
            // Round to appropriate precision
            let quantity = (quantity * 1000.0).ceil() / 1000.0;
            
            orders.push(OrderRequest {
                symbol: self.symbol.clone(),
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                quantity,
                price: Some(price),
                timeinforce: "GTC",
            });
            
            // println!("Created bid order: price={:.2}, quantity={:.3}", price, quantity);
        }
        
        // Create ask orders at different price levels
        for i in 1..=actual_levels {
            let price = mid_price + (i as f64 * self.spread);
            let price = (price / self.tick_size).round() * self.tick_size;
            
            // Ensure quantity meets minimum notional value
            let min_quantity = min_notional / price;
            let quantity = self.order_size.max(min_quantity);
            
            // Round to appropriate precision
            let quantity = (quantity * 1000.0).ceil() / 1000.0;
            
            orders.push(OrderRequest {
                symbol: self.symbol.clone(),
                side: OrderSide::Sell,
                order_type: OrderType::Limit,
                quantity,
                price: Some(price),
                timeinforce: "GTC",
            });
            
            // println!("Created ask order: price={:.2}, quantity={:.3}", price, quantity);
        }
        
        // Clone the orders before sending them
        let orders_clone = orders.clone();
        
        // Send the orders
        let responses = self.client.send_multiple_orders(orders).await?;
        
        // Track all orders
        for (i, response) in responses.iter().enumerate() {
            // println!("Tracking order: {}", response.order_id);
            
            if i < orders_clone.len() {
                ORDER_MANAGER.track_order("binance", &orders_clone[i], response)?;
            }
        }
        
        Ok(())
    }
    
    // Place skewed orders based on current position
    pub async fn place_skewed_orders(&self, mid_price: f64, levels: usize) -> Result<()> {
        println!("Placing skewed orders around mid price {}", mid_price);
        
        // Get current position
        let positions = self.status_checker.get_positions(&self.symbol).await?;
        
        // Calculate skew based on position
        let mut skew = 0.0;
        let max_position = 0.1; // Define your maximum desired position size
        
        for position in &positions {
            if position.symbol == self.symbol {
                // Normalize position to a value between -1.0 and 1.0
                skew = if position.side == "LONG" {
                    position.amount / max_position
                } else {
                    -position.amount / max_position
                };
                
                // Clamp skew between -1.0 and 1.0
                skew = skew.max(-1.0).min(1.0);
                break;
            }
        }
        
        println!("Current position skew: {:.2}", skew);
        
        // Calculate max orders based on available margin
        let max_orders = self.status_checker.calculate_max_orders(
            &self.symbol, mid_price, self.order_size).await?;
        
        // Determine how many levels we can afford
        let affordable_levels = max_orders / 2;
        let actual_levels = levels.min(affordable_levels);
        
        println!("Using {} levels (max affordable: {})", actual_levels, affordable_levels);
        
        // Calculate bid and ask levels based on skew
        // If skew is positive (long), we want more asks than bids
        // If skew is negative (short), we want more bids than asks
        let skew_factor = (1.0 - skew.abs()) * 0.5 + 0.5; // Maps to range [0.5, 1.0]
        
        let bid_levels = if skew > 0.0 {
            // We're long, reduce bids
            (actual_levels as f64 * skew_factor).round() as usize
        } else {
            // We're short or neutral, normal or increased bids
            actual_levels
        };
        
        let ask_levels = if skew < 0.0 {
            // We're short, reduce asks
            (actual_levels as f64 * skew_factor).round() as usize
        } else {
            // We're long or neutral, normal or increased asks
            actual_levels
        };
        
        println!("Using {} bid levels and {} ask levels", bid_levels, ask_levels);
        
        // Create a vector to hold all orders
        let mut orders = Vec::with_capacity(bid_levels + ask_levels);
        
        // Round mid price to tick size
        let mid_price = (mid_price / self.tick_size).round() * self.tick_size;
        
        // Calculate minimum quantity to meet notional value requirement
        let min_notional = 100.0; // Binance requires minimum notional of 100 USDT
        
        // Create bid orders at different price levels
        for i in 1..=bid_levels {
            let price = mid_price - (i as f64 * self.spread);
            let price = (price / self.tick_size).round() * self.tick_size;
            
            // Ensure quantity meets minimum notional value
            let min_quantity = min_notional / price;
            let quantity = self.order_size.max(min_quantity);
            
            // Round to appropriate precision
            let quantity = (quantity * 1000.0).ceil() / 1000.0;
            
            orders.push(OrderRequest {
                symbol: self.symbol.clone(),
                side: OrderSide::Buy,
                order_type: OrderType::Limit,
                quantity,
                price: Some(price),
                timeinforce: "GTC",
            });
            
            // println!("Created bid order: price={:.2}, quantity={:.3}", price, quantity);
        }
        
        // Create ask orders at different price levels
        for i in 1..=ask_levels {
            let price = mid_price + (i as f64 * self.spread);
            let price = (price / self.tick_size).round() * self.tick_size;
            
            // Ensure quantity meets minimum notional value
            let min_quantity = min_notional / price;
            let quantity = self.order_size.max(min_quantity);
            
            // Round to appropriate precision
            let quantity = (quantity * 1000.0).ceil() / 1000.0;
            
            orders.push(OrderRequest {
                symbol: self.symbol.clone(),
                side: OrderSide::Sell,
                order_type: OrderType::Limit,
                quantity,
                price: Some(price),
                timeinforce: "GTC",
            });
            
            // println!("Created ask order: price={:.2}, quantity={:.3}", price, quantity);
        }
        
        // Clone the orders before sending them
        let orders_clone = orders.clone();
        
        // Send the orders
        let responses = self.client.send_multiple_orders(orders).await?;
        
        // Track all orders
        for (i, response) in responses.iter().enumerate() {
            println!("Tracking order: {}", response.order_id);
            
            if i < orders_clone.len() {
                ORDER_MANAGER.track_order("binance", &orders_clone[i], response)?;
            }
        }
        
        Ok(())
    }
}