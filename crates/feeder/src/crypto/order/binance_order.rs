// File: src/exchanges/binance_orders.rs
use crate::error::{Result, Error};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde_urlencoded;
use hex;
use crate::core::{OrderExecutor, OrderRequest, OrderResponse, OrderSide, OrderType};
use async_trait::async_trait;
use reqwest::{Client, ClientBuilder};
use chrono::Utc;   
use std::time::Duration;
use futures::future::join_all;


type HmacSha256 = Hmac<Sha256>;
pub struct BinanceOrderClient {
    pub client: Client,
    pub api_key: String,
    pub secret_key: String,
    pub base_url: String, // Example: "https://api.binance.com"
}


impl BinanceOrderClient {
    pub fn new(api_key: String, secret_key: String, base_url: String) -> Self {
        // Create a client with connection pooling and keep-alive
        let client = ClientBuilder::new()
            .pool_idle_timeout(Some(Duration::from_secs(300)))
            .pool_max_idle_per_host(10)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .build()
            .expect("Failed to build HTTP client");
            
        BinanceOrderClient {
            client,
            api_key,
            secret_key,
            base_url,
        }
    }
}

#[async_trait]
impl OrderExecutor for BinanceOrderClient {

    async fn send_order(&self, order: OrderRequest) -> Result<OrderResponse> {
        // Collect your order parameters.
        let timestamp = Utc::now().timestamp_millis();
        // Create a vector of key-value parameters. Adjust keys according to the API docs.
        let mut params = vec![
            ("symbol", order.symbol.clone()),
            ("side", match order.side {
                OrderSide::Buy => "BUY".to_string(),
                OrderSide::Sell => "SELL".to_string(),
            }),
            ("type", match order.order_type {
                OrderType::Market => "MARKET".to_string(),
                OrderType::Limit => "LIMIT".to_string(),
            }),
            // ("price", order.price.unwrap().to_string()),
            ("quantity", order.quantity.to_string()),
            ("timestamp", timestamp.to_string()),
            ("timeInForce", order.timeinforce.to_string()),
        ];
        
        // Add price parameter only for limit orders
        if let OrderType::Limit = order.order_type {
            if let Some(price) = order.price {
                params.push(("price", price.to_string()));
            } else {
                return Err(Error::InvalidInput("Limit orders require a price".into()));
            }
        }

        // Sort and serialize parameters to a query string.
        let query = serde_urlencoded::to_string(&params).unwrap();
        
        // Generate HMAC-SHA256 signature.
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            let result = mac.finalize().into_bytes();
            hex::encode(result)
        };
        
        // Append the signature to the parameters.
        params.push(("signature", signature));
        let order_url = format!("{}{}", self.base_url, "/fapi/v1/order"); // or appropriate endpoint

        // Now, send the request.
        let response = self.client
            .post(&order_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .form(&params)
            .send()
            .await?;
                
            let resp_json: serde_json::Value = response.json().await?;
            // println!("Binance order response: {:?}", resp_json);
            
            // Process and map the JSON response to your OrderResponse.
            if let (Some(order_id), Some(status)) = (resp_json.get("orderId"), resp_json.get("status")) {
                Ok(OrderResponse {
                    order_id: order_id.to_string(),
                    status: status.to_string(),
                    price: resp_json.get("price")
                        .and_then(|p| p.as_str())
                        .and_then(|s| s.parse::<f64>().ok()),
                    filled_quantity: None,
                })
            } else {
                Err(Error::InvalidInput(format!(
                    "Invalid response from Binance order. Missing required fields. Response: {:?}",
                    resp_json
                )))
            }
        }
    
    async fn cancel_order(&self, order_id: &str) -> Result<OrderResponse> {
        // Create timestamp for the request
        let timestamp = Utc::now().timestamp_millis();

        // Create parameters for the cancel request
        let mut params = vec![
            ("symbol", "BTCUSDT".to_string()),  // Need symbol for cancellation
            ("orderId", order_id.to_string()),
            ("timestamp", timestamp.to_string()),
        ];

        // Create signature for the request
        let query = serde_urlencoded::to_string(&params).unwrap();
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            let result = mac.finalize().into_bytes();
            hex::encode(result)
        };

        // Add signature to parameters
        params.push(("signature", signature));

        // Send the cancel request
        let cancel_url = format!("{}/fapi/v1/order", self.base_url);
        let resp = self.client
            .delete(&cancel_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .form(&params)
            .send()
            .await?;

        let resp_json: serde_json::Value = resp.json().await?;
        // println!("Binance cancel response: {:?}", resp_json);

        // Parse the response
        if let (Some(order_id), Some(status)) = (resp_json.get("orderId"), resp_json.get("status")) {
            Ok(OrderResponse {
                order_id: order_id.to_string(),
                status: status.to_string(),
                price: resp_json.get("price")
                    .and_then(|p| p.as_str())
                    .and_then(|s| s.parse::<f64>().ok()),
                filled_quantity: resp_json.get("executedQty")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok()),
            })
        } else if let Some(msg) = resp_json.get("msg") {
            Err(Error::InvalidInput(format!("Binance error: {}", msg)))
        } else {
            Err(Error::InvalidInput("Invalid response from Binance cancel order".into()))
        }
    }


    async fn cancel_all_orders(&self, symbol: &str) -> Result<Vec<OrderResponse>> {
        let timestamp = Utc::now().timestamp_millis();
        
        let mut params = vec![
            ("symbol", symbol.to_string()),
            ("timestamp", timestamp.to_string()),
        ];
        
        let query = serde_urlencoded::to_string(&params).unwrap();
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            let result = mac.finalize().into_bytes();
            hex::encode(result)
        };
        
        params.push(("signature", signature));
        
        let cancel_all_url = format!("{}/fapi/v1/allOpenOrders", self.base_url);
        let resp = self.client
            .delete(&cancel_all_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .form(&params)
            .send()
            .await?;
            
        let resp_json: serde_json::Value = resp.json().await?;
        // println!("Cancel all orders response: {:?}", resp_json);
        
        // Parse the response into a vector of OrderResponse
        if let Some(orders) = resp_json.as_array() {
            let responses: Vec<OrderResponse> = orders
                .iter()
                .filter_map(|order| {
                    let order_id = order.get("orderId")?.to_string();
                    let status = order.get("status")?.to_string();
                    Some(OrderResponse {
                        order_id,
                        status,
                        price: order.get("price")
                            .and_then(|p| p.as_str())
                            .and_then(|s| s.parse::<f64>().ok()),
                        filled_quantity: order.get("executedQty")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok()),
                    })
                })
                .collect();
            Ok(responses)
        } else {
            Ok(vec![]) // Return empty vector if no orders were cancelled
        }
    }

    /// Place a limit order
    async fn place_limit_order(
        &self,
        symbol: &str,
        side: OrderSide,
        quantity: f64,
        price: f64
    ) -> Result<OrderResponse> {
        let order_request = OrderRequest {
            symbol: symbol.to_string(),
            side,
            order_type: OrderType::Limit,
            quantity,
            price: Some(price),
            timeinforce: "GTC",
        };
        
        self.send_order(order_request).await
    }
    
    /// Place a market order
    async fn place_market_order(
        &self,
        symbol: &str,
        side: OrderSide,
        quantity: f64
    ) -> Result<OrderResponse> {
        let order_request = OrderRequest {
            symbol: symbol.to_string(),
            side,
            order_type: OrderType::Market,
            quantity,
            price: None,
            timeinforce: "GTC",
        };
        
        self.send_order(order_request).await
    }

    async fn send_multiple_orders(&self, orders: Vec<OrderRequest>) -> Result<Vec<OrderResponse>> {
        // Create a vector of futures, each representing an order send operation
        let order_futures: Vec<_> = orders.into_iter()
            .map(|order| self.send_order(order))
            .collect();
            
        // Execute all order futures concurrently
        let results = join_all(order_futures).await;
        
        // Process results
        let mut responses = Vec::new();
        let mut errors = Vec::new();
        
        for result in results {
            match result {
                Ok(response) => {
                    // println!("Order placed successfully: {}", response.order_id);
                    responses.push(response);
                },
                Err(e) => {
                    println!("Failed to place order: {:?}", e);
                    errors.push(e);
                }
            }
        }
        
        // Decide how to handle errors - here we'll return an error if any order failed
        // Return detailed error information
        if !errors.is_empty() {
            return Err(Error::InvalidInput(format!(
                "Failed to place {} out of {} orders. First error: {:?}", 
                errors.len(), 
                errors.len() + responses.len(),
                errors.first().unwrap()
            )));
        }
        
        Ok(responses)
    }

}