// File: src/bin/order_speed_test.rs
use std::sync::Arc;
use std::time::Instant;
use futures::future::join_all;
use reqwest::Client;
use tokio;
use feeder::core::{OrderRequest, OrderSide, OrderType, 
                        OrderResponse, OrderExecutor, OrderStatusChecker, Position};
use feeder::core::{OrderManager, TrackedOrder, ORDER_MANAGER};

use feeder::error::Result;
use feeder::crypto::order::binance_order::BinanceOrderClient;
use feeder::crypto::order::binance_order_status::BinanceRestOrderStatusChecker;

#[tokio::main]
async fn main() -> Result<()> {
    // Wrap the BinanceOrderClient in an Arc so it can be shared between concurrent tasks.
    let client = Arc::new(BinanceOrderClient {
        client: Client::new(),
        api_key: "6fe678021bbfb1b384dbdfae8826d8d40f79b20416b98b4e220e287c18131967".to_string(),
        secret_key: "6de01865ed20b0693b796602ada0b968e5a76eeceb77cbd6758e4abc3229d7c7".to_string(),
        // Use your test server endpoint if available (e.g. "https://testnet.binance.vision")
        base_url: "https://testnet.binancefuture.com".to_string(),
    });

    // Create the status checker for querying order status
    let status_checker = Arc::new(BinanceRestOrderStatusChecker {
        client: Client::new(),
        api_key: "6fe678021bbfb1b384dbdfae8826d8d40f79b20416b98b4e220e287c18131967".to_string(),
        secret_key: "6de01865ed20b0693b796602ada0b968e5a76eeceb77cbd6758e4abc3229d7c7".to_string(),
        base_url: "https://testnet.binancefuture.com".to_string(),
    });

    // Symbol we want to trade
    let symbol = "BTCUSDT";
    
    // Step 1: Check for open orders and cancel them if needed
    println!("Checking for open orders...");
    let open_orders = status_checker.get_open_orders(symbol).await?;
    
    if !open_orders.is_empty() {
        println!("Found {} open orders, cancelling them...", open_orders.len());
        
        // Cancel all open orders
        let cancel_results = client.cancel_all_orders(symbol).await?;
        println!("Cancelled {} orders", cancel_results.len());
        
        // Update order manager with cancelled orders
        for response in &cancel_results {
            if let Some(_) = ORDER_MANAGER.get_order_details(&response.order_id) {
                ORDER_MANAGER.update_order(&response.order_id, response)?;
            }
        }
    } else {
        println!("No open orders found");
    }
    
    // Step 2: Check for open positions
    println!("Checking for open positions...");
    let positions = status_checker.get_positions(symbol).await?;
    
    if !positions.is_empty() {
        println!("Found {} open positions:", positions.len());
        for position in &positions {
            println!("  Symbol: {}, Side: {}, Amount: {}, Entry Price: {}", 
                position.symbol, position.side, position.amount, position.entry_price);
        }
        
        // Close existing positions if needed
        println!("Closing existing positions...");
        for position in &positions {
            // Create an order to close the position
            let close_side = if position.side == "LONG" { OrderSide::Sell } else { OrderSide::Buy };
            
            let close_request = OrderRequest {
                symbol: position.symbol.clone(),
                side: close_side,
                order_type: OrderType::Market,
                quantity: position.amount,
                price: None,
                timeinforce: "GTC",
            };
            
            println!("Sending close order: {:?}", close_request);
            match client.send_order(close_request.clone()).await {
                Ok(response) => {
                    println!("Position closed successfully: {}", response.order_id);
                    ORDER_MANAGER.track_order("binance", &close_request, &response)?;
                },
                Err(e) => {
                    eprintln!("Error closing position: {:?}", e);
                }
            }
        }
        
        // Wait for positions to be closed
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    } else {
        println!("No open positions found");
    }
    
    // Step 3: Send a new order
    let order_request = OrderRequest {
        symbol: symbol.to_string(),
        side: OrderSide::Buy,
        order_type: OrderType::Market,
        quantity: 0.01,
        price: None,
        timeinforce: "GTC",
    };
    
    println!("Sending new order: {:?}", order_request);
    let response = client.send_order(order_request.clone()).await?;
    println!("Order sent successfully: {}", response.order_id);
    
    // Track the order in the order manager
    ORDER_MANAGER.track_order("binance", &order_request, &response)?;
    
    // Step 4: Verify the order status
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    let updated_status = status_checker.query_order_status("binance", symbol, &response.order_id).await?;
    println!("Updated order status: {:?}", updated_status);
    
    // Update in order manager
    ORDER_MANAGER.update_order(&response.order_id, &updated_status)?;
    
    // Step 5: Check the new position
    let new_positions = status_checker.get_positions(symbol).await?;
    if !new_positions.is_empty() {
        println!("New positions after order:");
        for position in &new_positions {
            println!("  Symbol: {}, Side: {}, Amount: {}, Entry Price: {}", 
                position.symbol, position.side, position.amount, position.entry_price);
        }
    } else {
        println!("No positions found after order (may still be processing)");
    }
    
    // Step 6: List all tracked orders
    let all_orders = ORDER_MANAGER.get_all_orders();
    println!("All tracked orders: {}", all_orders.len());
    for order in all_orders {
        println!("Order ID: {}, Exchange: {}, Symbol: {}, Status: {}", 
            order.order_id, order.exchange_id, order.symbol, order.latest_response.status);
    }
    
    Ok(())
}