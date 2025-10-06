// File: src/oms/binance_rest_order_status.rs
use async_trait::async_trait;
use reqwest::Client;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::Value;
use crate::error::{Result, Error};
use crate::core::{OrderStatusChecker, OrderResponse, Position, 
                OrderSide, OrderRequest, OrderType, OrderExecutor, Balance};
use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use chrono::Utc;
use hex;

type HmacSha256 = Hmac<Sha256>;
pub struct BinanceRestOrderStatusChecker {
    pub client: Client,
    pub api_key: String,
    pub secret_key: String,
    pub base_url: String, // e.g. "https://api.binance.com"
}


#[async_trait]
impl OrderStatusChecker for BinanceRestOrderStatusChecker {
    // Add detailed error logging to the get_order_status method
    async fn query_order_status(&self, exchange: &str, symbol: &str, order_id: &str) -> Result<OrderResponse> {
        if exchange != "binance" {
            return Err(Error::InvalidInput(format!("Unsupported exchange: {}", exchange)));
        }
        
        let timestamp = Utc::now().timestamp_millis();
        
        // Create parameters for the request
        let params = vec![
            ("symbol", symbol.to_string()),
            ("orderId", order_id.to_string()),
            ("timestamp", timestamp.to_string()),
        ];
        
        // Generate the query string
        let query = serde_urlencoded::to_string(&params).unwrap();
        
        // Generate the signature
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };
        
        // Build the URL
        let url = format!("{}/fapi/v1/order", self.base_url); // futures
        // let url = format!("{}/api/v3/order", self.base_url); // spot
        
        // println!("Sending order status request to: {}", url);
        // println!("Query parameters: {}", query);
        
        let url_with_query = format!("{}?{}&signature={}", url, query, signature);

        // Send the request
        let resp = self.client.get(&url_with_query)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;
        
        let status = resp.status();
        let text = resp.text().await?;
        
        // println!("Binance API response status: {}", status);
        // println!("Binance API response body: {}", text);
        
        if !status.is_success() {
            return Err(Error::InvalidInput(format!(
                "Error from Binance API: Status {}, Body: {}", status, text
            )));
        }
        
        // Parse the response
        let resp_json: serde_json::Value = serde_json::from_str(&text)?;
        
        // Check for error
        if let Some(code) = resp_json.get("code") {
            let msg = resp_json.get("msg").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(Error::InvalidInput(format!(
                "Binance API error: code={}, msg={}", code, msg
            )));
        }
        
        // Extract the order details
        let order_id = resp_json.get("orderId")
        .and_then(|id| {
            if id.is_string() {
                id.as_str().map(|s| s.to_string())
            } else if id.is_number() {
                Some(id.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| Error::InvalidInput("Order ID not found in response".into()))?;
        
        let status = resp_json.get("status")
        .or_else(|| resp_json.get("orderStatus"))
        .and_then(|s| {
            if s.is_string() {
                s.as_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| Error::InvalidInput(format!(
            "Order status not found in response: {}", text
        )))?;
    
        let filled_qty = resp_json.get("executedQty")
        .or_else(|| resp_json.get("executed_qty"))
        .or_else(|| resp_json.get("filledQty"))
        .and_then(|q| {
            if q.is_string() {
                q.as_str().and_then(|s| s.parse::<f64>().ok())
            } else if q.is_number() {
                q.as_f64()
            } else {
                None
            }
        })
        .unwrap_or(0.0);

        // Map Binance status to our OrderResponse status
        let status = match status.as_str() {
            "NEW" | "PARTIALLY_FILLED" => "OPEN",
            "FILLED" => "FILLED",
            "CANCELED" | "REJECTED" | "EXPIRED" => "CANCELED",
            _ => "UNKNOWN",
        };
        
        Ok(OrderResponse {
            order_id: order_id.to_string(),
            status: status.to_string(),
            filled_quantity: Some(filled_qty),
            price: None,
        })
    }

    async fn get_open_orders(&self, symbol: &str) -> Result<Vec<OrderResponse>> {
        let timestamp = Utc::now().timestamp_millis();
            
        let mut params = vec![
            ("symbol", symbol.to_string()),
            ("orderId", "1234567890".to_string()),
            ("timestamp", timestamp.to_string()),
            // ("recvWindow", "60000".to_string()),  // Add recvWindow parameter
        ];
        
        // Sort parameters before signing (Binance requirement)
        // params.sort_by(|a, b| a.0.cmp(b.0));        

        let query = serde_urlencoded::to_string(&params).unwrap();
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            let result = mac.finalize().into_bytes();
            hex::encode(result)
        };
        
        params.push(("signature", signature));
                
        let open_orders_url = format!("{}/fapi/v1/openOrders", self.base_url);
        // println!("Requesting open orders from URL: {}", open_orders_url);
        // println!("API Key (first 10 chars): {}", &self.api_key[..10]);

        // println!("With params: {:?}", params);

        let response = self.client
                                .get(&open_orders_url)
                                .header("X-MBX-APIKEY", &self.api_key)
                                .query(&params)
                                .send()
                                .await?
                                .json::<Vec<serde_json::Value>>()  // Changed to directly parse as Vec
                                .await?;

        // // Print response status and headers
        // println!("Response status: {}", resp.status());
        
        // // Only print specific headers we care about
        // if let Some(content_type) = resp.headers().get("content-type") {
        // println!("Content-Type: {:?}", content_type);
        // }

        // let resp_json: serde_json::Value = resp.json().await?;
        // println!("Open orders response: {:?}", resp_json);
        
        let responses = response.iter()
        .map(|order| {
            OrderResponse {
                order_id: order.get("orderId")
                    .and_then(|v| Some(v.to_string()))
                    .unwrap_or_default(),
                status: order.get("status")
                    .and_then(|v| Some(v.to_string()))
                    .unwrap_or_default(),
                price: order.get("price")
                    .and_then(|p| p.as_str())
                    .and_then(|s| s.parse::<f64>().ok()),
                filled_quantity: order.get("executedQty")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok()),
            }
        })
        .collect();
        Ok(responses)
    }

    async fn get_positions(&self, symbol: &str) -> Result<Vec<Position>> {
        let timestamp = Utc::now().timestamp_millis();
        
        let mut params = vec![
            ("timestamp", timestamp.to_string()),
        ];
        
        // Add symbol if provided (otherwise get all positions)
        if !symbol.is_empty() {
            params.push(("symbol", symbol.to_string()));
        }
        
        let query = serde_urlencoded::to_string(&params).unwrap();
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            let result = mac.finalize().into_bytes();
            hex::encode(result)
        };
        
        params.push(("signature", signature));
        
        // Binance Futures endpoint for positions
        let positions_url = format!("{}/fapi/v2/positionRisk", self.base_url);
        
        let resp = self.client
            .get(&positions_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&params)
            .send()
            .await?;
            
        let positions_json: Vec<serde_json::Value> = resp.json().await?;
        
        // Parse the response into Position objects
        let positions = positions_json.into_iter()
            .filter_map(|pos| {
                let symbol = pos.get("symbol")?.as_str()?.to_string();
                
                // Skip if we're filtering by symbol and this doesn't match
                if !symbol.is_empty() && symbol != symbol {
                    return None;
                }
                
                let position_amt_str = pos.get("positionAmt")?.as_str()?;
                let position_amt = position_amt_str.parse::<f64>().ok()?;
                
                // Only include non-zero positions
                if position_amt == 0.0 {
                    return None;
                }
                
                let entry_price = pos.get("entryPrice")?.as_str()?.parse().ok()?;
                let unrealized_profit = pos.get("unRealizedProfit")?.as_str()?.parse().ok()?;
                
                // Determine side based on position amount
                let side = if position_amt > 0.0 { "LONG".to_string() } else { "SHORT".to_string() };
                
                Some(Position {
                    symbol,
                    amount: position_amt.abs(),
                    entry_price,
                    unrealized_profit,
                    side,
                })
            })
            .collect();
        
        Ok(positions)
    }

    /// Close a specific position
    async fn close_position(
        &self,
        position: &Position,
        executor: &Arc<dyn OrderExecutor>
    ) -> Result<OrderResponse> {
        // Determine the side to close the position
        let close_side = if position.side == "LONG" { OrderSide::Sell } else { OrderSide::Buy };
        
        let close_request = OrderRequest {
            symbol: position.symbol.clone(),
            side: close_side,
            order_type: OrderType::Market,
            quantity: position.amount,
            price: None,
            timeinforce: "GTC",
        };
        
        executor.send_order(close_request).await
    }    

    async fn get_account_balance(&self) -> Result<Vec<Balance>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
            
        let mut params = vec![("timestamp", timestamp.to_string())];
        
        let query = serde_urlencoded::to_string(&params).unwrap();
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };
        
        params.push(("signature", signature));
        
        let url = format!("{}/fapi/v2/balance", self.base_url);
        
        let response = self.client.get(&url)
            .query(&params)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;
            
        let data: Vec<Value> = response.json().await?;
        
        let balances = data.iter()
            .filter_map(|asset| {
                let asset_name = asset["asset"].as_str()?.to_string();
                let amount = asset["availableBalance"].as_str()?
                    .parse::<f64>().ok()?;
                
                Some(Balance {
                    asset: asset_name,
                    amount,
                })
            })
            .collect();
        
        Ok(balances)
    }
    
    async fn calculate_max_orders(&self, symbol: &str, price: f64, size: f64) -> Result<usize> {
        // Get account balance
        let balances = self.get_account_balance().await?;
        
        // For futures, we typically use USDT as margin
        let available_margin = balances.iter()
            .find(|b| b.asset == "USDT")
            .map(|b| b.amount)
            .unwrap_or(0.0);
        
        println!("Available margin: {} USDT", available_margin);
        
        // Get account information to determine leverage
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
            
        let mut params = vec![("timestamp", timestamp.to_string())];
        
        // Add symbol if provided
        if !symbol.is_empty() {
            params.push(("symbol", symbol.to_string()));
        }
        
        let query = serde_urlencoded::to_string(&params).unwrap();
        let signature = {
            let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(query.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };
        
        params.push(("signature", signature));
        
        let url = format!("{}/fapi/v1/leverageBracket", self.base_url);
        
        let response = self.client.get(&url)
            .query(&params)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;
            
        let data: Vec<Value> = response.json().await?;
        
        // Find the leverage for this symbol
        let mut leverage = 1.0; // Default leverage
        for item in data {
            if let Some(sym) = item["symbol"].as_str() {
                if sym == symbol {
                    if let Some(brackets) = item["brackets"].as_array() {
                        if let Some(first) = brackets.first() {
                            leverage = first["initialLeverage"].as_f64().unwrap_or(1.0);
                        }
                    }
                    break;
                }
            }
        }
        
        println!("Using leverage: {}", leverage);
        
        // Calculate margin required per order
        let order_value = price * size;
        let margin_per_order = order_value / leverage;
        
        println!("Margin required per order: {} USDT", margin_per_order);
        
        // Calculate max number of orders (using 80% of available margin to be safe)
        let safe_margin = available_margin * 0.8;
        let max_orders = (safe_margin / margin_per_order) as usize;
        
        println!("Maximum number of orders possible: {}", max_orders);
        
        Ok(max_orders)
    }
    
}