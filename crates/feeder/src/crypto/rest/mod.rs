/// REST API module for cryptocurrency exchanges
///
/// This module provides REST API clients for various exchanges, supporting:
/// - Market data (orderbook snapshots, trades, tickers)
/// - Account management (balances, positions)
/// - Order management (place, cancel, modify orders)
/// - Historical data queries

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use reqwest::{Client, Response};
use std::time::Duration;
use std::collections::HashMap;

// Re-export all exchange REST clients
pub mod binance;
pub mod bybit;
pub mod okx;
pub mod coinbase;
pub mod deribit;
pub mod upbit;
pub mod bithumb;

pub use binance::BinanceRestClient;
pub use bybit::BybitRestClient;
pub use okx::OkxRestClient;
pub use coinbase::CoinbaseRestClient;
pub use deribit::DeribitRestClient;
pub use upbit::UpbitRestClient;
pub use bithumb::BithumbRestClient;

/// Rate limiter to prevent exceeding exchange limits
pub struct RateLimiter {
    last_request: std::time::Instant,
    min_interval: Duration,
}

impl RateLimiter {
    pub fn new(requests_per_second: u32) -> Self {
        Self {
            last_request: std::time::Instant::now() - Duration::from_secs(1),
            min_interval: Duration::from_millis(1000 / requests_per_second as u64),
        }
    }

    pub async fn wait(&mut self) {
        let elapsed = self.last_request.elapsed();
        if elapsed < self.min_interval {
            tokio::time::sleep(self.min_interval - elapsed).await;
        }
        self.last_request = std::time::Instant::now();
    }
}

/// Common orderbook snapshot structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub symbol: String,
    pub bids: Vec<(f64, f64)>,  // (price, quantity)
    pub asks: Vec<(f64, f64)>,  // (price, quantity)
    pub timestamp: u64,
    pub last_update_id: Option<u64>,  // For exchanges that provide sequence numbers
}

/// Common trade structure for REST API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: String,
    pub price: f64,
    pub quantity: f64,
    pub timestamp: u64,
    pub is_buyer_maker: bool,
    pub trade_id: Option<String>,
}

/// Common balance structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub free: f64,
    pub locked: f64,
    pub total: f64,
}

/// Order types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
    StopLoss,
    StopLossLimit,
    TakeProfit,
    TakeProfitLimit,
}

/// Order side
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Order status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

/// Common order structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub symbol: String,
    pub order_id: String,
    pub client_order_id: Option<String>,
    pub price: Option<f64>,
    pub quantity: f64,
    pub executed_quantity: f64,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub timestamp: u64,
}

/// Common trait for all exchange REST clients
#[async_trait]
pub trait ExchangeRestClient: Send + Sync {
    /// Get exchange name
    fn name(&self) -> &str;

    /// Get orderbook snapshot
    async fn get_orderbook(&self, symbol: &str, depth: Option<u32>) -> Result<OrderBookSnapshot>;

    /// Get recent trades
    async fn get_recent_trades(&self, symbol: &str, limit: Option<u32>) -> Result<Vec<Trade>>;

    /// Get account balances (requires authentication)
    async fn get_balances(&self) -> Result<Vec<Balance>>;

    /// Place order (requires authentication)
    async fn place_order(
        &self,
        symbol: &str,
        side: OrderSide,
        order_type: OrderType,
        quantity: f64,
        price: Option<f64>,
    ) -> Result<Order>;

    /// Cancel order (requires authentication)
    async fn cancel_order(&self, symbol: &str, order_id: &str) -> Result<Order>;

    /// Get order status (requires authentication)
    async fn get_order(&self, symbol: &str, order_id: &str) -> Result<Order>;

    /// Get open orders (requires authentication)
    async fn get_open_orders(&self, symbol: Option<&str>) -> Result<Vec<Order>>;
}

/// Base REST client with common functionality
pub struct BaseRestClient {
    pub client: Client,
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub rate_limiter: RateLimiter,
}

impl BaseRestClient {
    pub fn new(base_url: &str, requests_per_second: u32) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.to_string(),
            api_key: None,
            api_secret: None,
            rate_limiter: RateLimiter::new(requests_per_second),
        }
    }

    pub fn with_credentials(mut self, api_key: String, api_secret: String) -> Self {
        self.api_key = Some(api_key);
        self.api_secret = Some(api_secret);
        self
    }

    /// Make GET request with rate limiting
    pub async fn get(&mut self, endpoint: &str, params: Option<HashMap<String, String>>) -> Result<Response> {
        self.rate_limiter.wait().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let mut request = self.client.get(&url);

        if let Some(params) = params {
            request = request.query(&params);
        }

        Ok(request.send().await?)
    }

    /// Make POST request with rate limiting
    pub async fn post(&mut self, endpoint: &str, body: Option<HashMap<String, String>>) -> Result<Response> {
        self.rate_limiter.wait().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let mut request = self.client.post(&url);

        if let Some(body) = body {
            request = request.json(&body);
        }

        Ok(request.send().await?)
    }

    /// Make DELETE request with rate limiting
    pub async fn delete(&mut self, endpoint: &str, params: Option<HashMap<String, String>>) -> Result<Response> {
        self.rate_limiter.wait().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let mut request = self.client.delete(&url);

        if let Some(params) = params {
            request = request.query(&params);
        }

        Ok(request.send().await?)
    }

    /// Generate HMAC SHA256 signature
    pub fn sign(&self, message: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        if let Some(secret) = &self.api_secret {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .expect("HMAC can take key of any size");
            mac.update(message.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        } else {
            String::new()
        }
    }
}

/// Get REST client for specific exchange
pub fn get_rest_client(exchange: &str, api_key: Option<String>, api_secret: Option<String>) -> Result<Box<dyn ExchangeRestClient>> {
    let client: Box<dyn ExchangeRestClient> = match exchange.to_lowercase().as_str() {
        "binance" => {
            let mut client = Box::new(BinanceRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        "bybit" => {
            let mut client = Box::new(BybitRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        "okx" => {
            let mut client = Box::new(OkxRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        "coinbase" => {
            let mut client = Box::new(CoinbaseRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        "deribit" => {
            let mut client = Box::new(DeribitRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        "upbit" => {
            let mut client = Box::new(UpbitRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        "bithumb" => {
            let mut client = Box::new(BithumbRestClient::new());
            if let (Some(key), Some(secret)) = (api_key, api_secret) {
                client = Box::new(client.with_credentials(key, secret));
            }
            client
        },
        _ => return Err(anyhow::anyhow!("Unknown exchange: {}", exchange)),
    };

    Ok(client)
}