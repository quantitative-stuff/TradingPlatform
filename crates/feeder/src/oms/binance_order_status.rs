// File: src/oms/binance_rest_order_status.rs
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use crate::error::Result;
use crate::core::{OrderStatusChecker, OrderResponse};

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
    async fn query_order_status(&self, order_id: &str) -> Result<OrderResponse> {
        let url = format!("{}/fapi/v1/order", self.base_url);
        let resp = self.client.get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("orderId", order_id)])
            .send().await?;
        let resp_json: serde_json::Value = resp.json().await?;
        if let (Some(order_id_val), Some(status_val)) = (resp_json.get("orderId"), resp_json.get("status")) {
            Ok(OrderResponse {
                
                order_id: order_id_val.to_string(),
                status: status_val.to_string(),
                // Uncomment and extend here if you need extra fields:
                filled_quantity: resp_json.get("executedQty")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok()),
            })
        } else {
            Err(crate::error::Error::InvalidInput("Invalid response from REST order status API".into()))        }
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
        println!("Requesting open orders from URL: {}", open_orders_url);
        println!("API Key (first 10 chars): {}", &self.api_key[..10]);

        println!("With params: {:?}", params);

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
        .filter_map(|order| {
            let order_id = order.get("orderId")?.to_string();
            let status = order.get("status")?.to_string();
            Some(OrderResponse {
                order_id,
                status,
                filled_quantity: order.get("executedQty")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok()),
            })
        })
        .collect();
        Ok(responses)
    }
}