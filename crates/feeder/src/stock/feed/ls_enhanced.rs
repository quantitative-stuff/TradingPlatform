use async_trait::async_trait;
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::core::{TradeData, OrderBookData, get_optimized_udp_sender, CONNECTION_STATS};
use crate::error::{Result, Error};
use crate::load_config_ls::LSConfig;
use crate::stock::feed::FeederStock;
use tracing::{info, warn, error, debug};

// Enhanced Asset Type Support
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AssetType {
    Stock,
    Futures,
    Options,
    OverseasFutures,
    ETF,
    ELW,
}

impl AssetType {
    fn as_str(&self) -> &str {
        match self {
            AssetType::Stock => "stock",
            AssetType::Futures => "futures",
            AssetType::Options => "options",
            AssetType::OverseasFutures => "overseas_futures",
            AssetType::ETF => "etf",
            AssetType::ELW => "elw",
        }
    }
}

// Market Code Enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketCode {
    KOSPI,
    KOSDAQ,
    KOTC,
    Futures,
    Options,
}

impl MarketCode {
    fn code(&self) -> &str {
        match self {
            MarketCode::KOSPI => "1",
            MarketCode::KOSDAQ => "2",
            MarketCode::KOTC => "3",
            MarketCode::Futures => "4",
            MarketCode::Options => "5",
        }
    }
}

// Options Data with Greeks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionsData {
    pub symbol: String,
    pub underlying: String,
    pub strike_price: f64,
    pub expiry_date: String,
    pub option_type: OptionType,
    pub price: f64,
    pub implied_volatility: f64,
    pub greeks: OptionsGreeks,
    pub open_interest: i64,
    pub volume: i64,
    pub timestamp: u64,
    pub timestamp_unit: crate::load_config::TimestampUnit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionsGreeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
    pub rho: f64,
}

// Futures Data with Open Interest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesData {
    pub symbol: String,
    pub underlying: String,
    pub expiry_date: String,
    pub price: f64,
    pub open_interest: i64,
    pub volume: i64,
    pub basis: f64,
    pub settlement_price: f64,
    pub timestamp: u64,
    pub timestamp_unit: crate::load_config::TimestampUnit,
}

// ETF Data with NAV
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ETFData {
    pub symbol: String,
    pub name: String,
    pub nav: f64,
    pub tracking_error: f64,
    pub expense_ratio: f64,
    pub benchmark: String,
    pub fund_size: f64,
    pub price: f64,
    pub premium_discount: f64,
    pub timestamp: i64,
}

// Transaction Codes for LS Securities API
pub struct TransactionCodes;

impl TransactionCodes {
    // Stock Market Data
    pub const STOCK_CURRENT_PRICE: &'static str = "t1101";
    pub const STOCK_TIME_SERIES: &'static str = "t1301";
    pub const STOCK_MASTER_LIST: &'static str = "t8436";
    
    // Futures Market Data
    pub const FUTURES_MASTER_LIST: &'static str = "t2301";
    pub const FUTURES_CURRENT_PRICE: &'static str = "t2101";
    pub const FUTURES_OPEN_INTEREST: &'static str = "t2501";
    pub const FUTURES_BASIS: &'static str = "t2601";
    
    // Options Market Data
    pub const OPTIONS_MASTER_LIST: &'static str = "t2301";
    pub const OPTIONS_CURRENT_PRICE: &'static str = "t2105";
    pub const OPTIONS_CHAIN: &'static str = "t2110";
    pub const OPTIONS_GREEKS: &'static str = "t2107";
    pub const OPTIONS_IMPLIED_VOL: &'static str = "t2108";
    
    // Overseas Futures
    pub const OVERSEAS_FUTURES_MASTER: &'static str = "o3101";
    pub const OVERSEAS_FUTURES_PRICE: &'static str = "o3103";
    
    // ETF/ELW
    pub const ETF_MASTER_LIST: &'static str = "t1404";
    pub const ETF_NAV_INFO: &'static str = "t1481";
    pub const ETF_PORTFOLIO: &'static str = "t1482";
    
    // Real-time WebSocket Codes
    pub const RT_STOCK_PRICE: &'static str = "S3_";
    pub const RT_STOCK_QUOTE: &'static str = "H1_";
    pub const RT_FUTURES_PRICE: &'static str = "FC_";
    pub const RT_FUTURES_QUOTE: &'static str = "FH_";
    pub const RT_FUTURES_OI: &'static str = "FO_";
    pub const RT_OPTIONS_PRICE: &'static str = "OC_";
    pub const RT_OPTIONS_QUOTE: &'static str = "OH_";
    pub const RT_OPTIONS_OI: &'static str = "OO_";
    pub const RT_OVERSEAS_FUTURES: &'static str = "OF_";
}

// Enhanced Derivative Info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivativeInfo {
    pub symbol: String,
    pub name: String,
    pub underlying: String,
    pub asset_type: AssetType,
    pub expiry_date: String,
    pub strike_price: Option<f64>,
    pub option_type: Option<OptionType>,
    pub multiplier: i32,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i32,
}

// Enhanced LS Exchange with full derivatives support
pub struct LSEnhancedExchange {
    config: LSConfig,
    access_token: Option<String>,
    token_expiry: Option<DateTime<Utc>>,
    ws_stream: Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    symbols_by_type: HashMap<AssetType, Vec<String>>,
    reconnect_count: u32,
    max_reconnect_attempts: u32,
    derivatives_cache: HashMap<String, DerivativeInfo>,
}

impl LSEnhancedExchange {
    const BASE_URL: &'static str = "https://openapi.ls-sec.co.kr:8080";
    const WS_URL: &'static str = "wss://openapi.ls-sec.co.kr:9443/websocket";

    pub fn new(config: LSConfig) -> Self {
        Self {
            config,
            access_token: None,
            token_expiry: None,
            ws_stream: None,
            symbols_by_type: HashMap::new(),
            reconnect_count: 0,
            max_reconnect_attempts: 10,
            derivatives_cache: HashMap::new(),
        }
    }

    pub fn add_symbols(&mut self, asset_type: AssetType, symbols: Vec<String>) {
        self.symbols_by_type.insert(asset_type, symbols);
    }

    async fn authenticate(&mut self) -> Result<()> {
        let client = reqwest::Client::new();
        
        let params = [
            ("grant_type", "client_credentials"),
            ("appkey", &self.config.app_key),
            ("appsecretkey", &self.config.secret_key),
            ("scope", "oob"),
        ];

        let response = client
            .post(format!("{}/oauth2/token", Self::BASE_URL))
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(Error::Auth(format!("Authentication failed: {}", error_text)));
        }

        let token_response: TokenResponse = response.json().await?;
        
        self.access_token = Some(token_response.access_token);
        self.token_expiry = Some(Utc::now() + chrono::Duration::seconds(token_response.expires_in as i64 - 60));
        
        info!("Successfully authenticated with LS Securities API");
        Ok(())
    }

    async fn check_token_validity(&mut self) -> Result<()> {
        if let Some(expiry) = self.token_expiry {
            if Utc::now() >= expiry {
                info!("Token expired, re-authenticating...");
                self.authenticate().await?;
            }
        } else {
            self.authenticate().await?;
        }
        Ok(())
    }

    async fn connect_websocket(&mut self) -> Result<()> {
        let (ws_stream, _) = connect_async(Self::WS_URL).await?;
        self.ws_stream = Some(ws_stream);
        info!("WebSocket connection established");
        Ok(())
    }

    fn create_enhanced_subscription(&self) -> Result<Vec<String>> {
        let access_token = self.access_token.as_ref()
            .ok_or_else(|| Error::Auth("No access token".into()))?;

        let mut messages = Vec::new();

        // Subscribe to different asset types with appropriate transaction codes
        for (asset_type, symbols) in &self.symbols_by_type {
            let subscription_codes = match asset_type {
                AssetType::Stock | AssetType::ETF => vec![
                    TransactionCodes::RT_STOCK_PRICE,
                    TransactionCodes::RT_STOCK_QUOTE,
                ],
                AssetType::Futures => vec![
                    TransactionCodes::RT_FUTURES_PRICE,
                    TransactionCodes::RT_FUTURES_QUOTE,
                    TransactionCodes::RT_FUTURES_OI,
                ],
                AssetType::Options => vec![
                    TransactionCodes::RT_OPTIONS_PRICE,
                    TransactionCodes::RT_OPTIONS_QUOTE,
                    TransactionCodes::RT_OPTIONS_OI,
                ],
                AssetType::OverseasFutures => vec![
                    TransactionCodes::RT_OVERSEAS_FUTURES,
                ],
                AssetType::ELW => vec![
                    TransactionCodes::RT_STOCK_PRICE,
                    TransactionCodes::RT_STOCK_QUOTE,
                ],
            };

            for symbol in symbols {
                for tr_cd in &subscription_codes {
                    let message = json!({
                        "header": {
                            "token": access_token,
                            "tr_type": "3"
                        },
                        "body": {
                            "tr_cd": tr_cd,
                            "tr_key": symbol
                        }
                    });
                    messages.push(message.to_string());
                }
            }
        }

        Ok(messages)
    }

    async fn process_enhanced_message(&self, message: Value) -> Result<()> {
        if let Some(header) = message.get("header") {
            if let Some(tr_cd) = header.get("tr_cd").and_then(|v| v.as_str()) {
                match tr_cd {
                    // Stock and ETF
                    tr if tr.starts_with("S3_") => {
                        self.handle_stock_etf_price(message, AssetType::Stock).await?;
                    },
                    tr if tr.starts_with("H1_") => {
                        self.handle_stock_etf_quote(message, AssetType::Stock).await?;
                    },
                    // Futures
                    tr if tr.starts_with("FC_") => {
                        self.handle_futures_price(message).await?;
                    },
                    tr if tr.starts_with("FH_") => {
                        self.handle_futures_quote(message).await?;
                    },
                    tr if tr.starts_with("FO_") => {
                        self.handle_futures_open_interest(message).await?;
                    },
                    // Options
                    tr if tr.starts_with("OC_") => {
                        self.handle_options_price(message).await?;
                    },
                    tr if tr.starts_with("OH_") => {
                        self.handle_options_quote(message).await?;
                    },
                    tr if tr.starts_with("OO_") => {
                        self.handle_options_open_interest(message).await?;
                    },
                    // Overseas Futures
                    tr if tr.starts_with("OF_") => {
                        self.handle_overseas_futures(message).await?;
                    },
                    _ => {
                        debug!("Unknown message type: {}", tr_cd);
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_stock_etf_price(&self, message: Value, asset_type: AssetType) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("shcode").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let price_f64 = body.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let quantity_f64 = body.get("cvolume").and_then(|v| v.as_f64()).unwrap_or(0.0);

            let trade_data = TradeData::from_f64(
                "LS".to_string(),
                symbol.clone(),
                asset_type.as_str().to_string(),
                price_f64,
                quantity_f64,
                8,  // price_precision
                8,  // quantity_precision
                chrono::Utc::now().timestamp_millis() as u64,
                crate::load_config::TimestampUnit::Milliseconds,
            );

            if let Some(sender) = get_optimized_udp_sender() {
                let _ = sender.send_trade_data(trade_data);
            }
        }
        Ok(())
    }

    async fn handle_stock_etf_quote(&self, message: Value, asset_type: AssetType) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("shcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            // Extract multiple levels of bid/ask
            let mut bids = Vec::new();
            let mut asks = Vec::new();
            
            for i in 1..=10 {
                let bid_price_key = format!("bidho{}", i);
                let bid_size_key = format!("bidrem{}", i);
                let ask_price_key = format!("offerho{}", i);
                let ask_size_key = format!("offerrem{}", i);
                
                if let (Some(bid_price), Some(bid_size)) = (
                    body.get(&bid_price_key).and_then(|v| v.as_f64()),
                    body.get(&bid_size_key).and_then(|v| v.as_f64())
                ) {
                    if bid_price > 0.0 {
                        bids.push((bid_price, bid_size));
                    }
                }
                
                if let (Some(ask_price), Some(ask_size)) = (
                    body.get(&ask_price_key).and_then(|v| v.as_f64()),
                    body.get(&ask_size_key).and_then(|v| v.as_f64())
                ) {
                    if ask_price > 0.0 {
                        asks.push((ask_price, ask_size));
                    }
                }
            }
            
            let orderbook_data = OrderBookData::from_f64(
                "LS".to_string(),
                symbol,
                asset_type.as_str().to_string(),
                bids,
                asks,
                8,  // price_precision
                8,  // quantity_precision
                chrono::Utc::now().timestamp_millis() as u64,
                crate::load_config::TimestampUnit::Milliseconds,
            );

            if let Some(sender) = get_optimized_udp_sender() {
                let _ = sender.send_orderbook_data(orderbook_data);
            }
        }
        Ok(())
    }

    async fn handle_futures_price(&self, message: Value) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("futcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            let futures_data = FuturesData {
                symbol: symbol.clone(),
                underlying: body.get("basecode").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                expiry_date: body.get("lastdate").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                price: body.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0),
                open_interest: body.get("openinterest").and_then(|v| v.as_i64()).unwrap_or(0),
                volume: body.get("volume").and_then(|v| v.as_i64()).unwrap_or(0),
                basis: body.get("basis").and_then(|v| v.as_f64()).unwrap_or(0.0),
                settlement_price: body.get("settlement").and_then(|v| v.as_f64()).unwrap_or(0.0),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                timestamp_unit: crate::load_config::TimestampUnit::default(),
            };

            // Convert to TradeData for UDP sending
            let trade_data = TradeData::from_f64(
                "LS".to_string(),
                symbol,
                "futures".to_string(),
                futures_data.price,
                futures_data.volume as f64,
                8,  // price_precision
                8,  // quantity_precision
                futures_data.timestamp,
                crate::load_config::TimestampUnit::Milliseconds,
            );

            if let Some(sender) = get_optimized_udp_sender() {
                let _ = sender.send_trade_data(trade_data);
            }
        }
        Ok(())
    }

    async fn handle_futures_quote(&self, message: Value) -> Result<()> {
        self.handle_stock_etf_quote(message, AssetType::Futures).await
    }

    async fn handle_futures_open_interest(&self, message: Value) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("futcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let open_interest = body.get("openinterest").and_then(|v| v.as_i64()).unwrap_or(0);
            
            debug!("Futures OI update - Symbol: {}, OI: {}", symbol, open_interest);
            // Store or process open interest data as needed
        }
        Ok(())
    }

    async fn handle_options_price(&self, message: Value) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("optcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            let options_data = OptionsData {
                symbol: symbol.clone(),
                underlying: body.get("basecode").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                strike_price: body.get("actprice").and_then(|v| v.as_f64()).unwrap_or(0.0),
                expiry_date: body.get("lastdate").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                option_type: if body.get("opttype").and_then(|v| v.as_str()) == Some("C") {
                    OptionType::Call
                } else {
                    OptionType::Put
                },
                price: body.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0),
                implied_volatility: body.get("impvol").and_then(|v| v.as_f64()).unwrap_or(0.0),
                greeks: OptionsGreeks {
                    delta: body.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    gamma: body.get("gamma").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    theta: body.get("theta").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    vega: body.get("vega").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    rho: body.get("rho").and_then(|v| v.as_f64()).unwrap_or(0.0),
                },
                open_interest: body.get("openinterest").and_then(|v| v.as_i64()).unwrap_or(0),
                volume: body.get("volume").and_then(|v| v.as_i64()).unwrap_or(0),
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                timestamp_unit: crate::load_config::TimestampUnit::default(),
            };

            // Convert to TradeData for UDP sending
            let trade_data = TradeData::from_f64(
                "LS".to_string(),
                symbol,
                "options".to_string(),
                options_data.price,
                options_data.volume as f64,
                8,  // price_precision
                8,  // quantity_precision
                options_data.timestamp,
                crate::load_config::TimestampUnit::Milliseconds,
            );

            if let Some(sender) = get_optimized_udp_sender() {
                let _ = sender.send_trade_data(trade_data);
            }
        }
        Ok(())
    }

    async fn handle_options_quote(&self, message: Value) -> Result<()> {
        self.handle_stock_etf_quote(message, AssetType::Options).await
    }

    async fn handle_options_open_interest(&self, message: Value) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("optcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let open_interest = body.get("openinterest").and_then(|v| v.as_i64()).unwrap_or(0);
            
            debug!("Options OI update - Symbol: {}, OI: {}", symbol, open_interest);
            // Store or process open interest data as needed
        }
        Ok(())
    }

    async fn handle_overseas_futures(&self, message: Value) -> Result<()> {
        if let Some(body) = message.get("body") {
            let symbol = body.get("symbol").and_then(|v| v.as_str()).unwrap_or("").to_string();
            
            let trade_data = TradeData::from_f64(
                "LS".to_string(),
                symbol.clone(),
                "overseas_futures".to_string(),
                body.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0),
                body.get("volume").and_then(|v| v.as_f64()).unwrap_or(0.0),
                8,  // price_precision
                8,  // quantity_precision
                chrono::Utc::now().timestamp_millis() as u64,
                crate::load_config::TimestampUnit::Milliseconds,
            );

            if let Some(sender) = get_optimized_udp_sender() {
                let _ = sender.send_trade_data(trade_data);
            }
        }
        Ok(())
    }

    // Fetch master data for all asset types
    pub async fn fetch_all_master_data(&mut self) -> Result<()> {
        self.check_token_validity().await?;
        
        info!("Fetching master data for all asset types...");
        
        // Fetch stock master
        self.fetch_stock_master().await?;
        
        // Fetch futures master
        self.fetch_futures_master().await?;
        
        // Fetch options master
        self.fetch_options_master().await?;
        
        // Fetch ETF master
        self.fetch_etf_master().await?;
        
        // Fetch overseas futures master
        self.fetch_overseas_futures_master().await?;
        
        info!("Master data fetching completed");
        Ok(())
    }

    async fn fetch_stock_master(&self) -> Result<()> {
        let client = reqwest::Client::new();
        let token = self.access_token.as_ref().ok_or(Error::Auth("No token".into()))?;
        
        let url = format!("{}/stock/market-data", Self::BASE_URL);
        let headers = json!({
            "content-type": "application/json; charset=utf-8",
            "authorization": format!("Bearer {}", token),
            "tr_cd": TransactionCodes::STOCK_MASTER_LIST,
            "tr_cont": "N",
            "tr_cont_key": ""
        });
        
        let body = json!({"t8436InBlock": {"gubun": "0"}});
        
        let response = client.post(&url)
            .json(&headers)
            .json(&body)
            .send()
            .await?;
        
        if response.status().is_success() {
            let data: Value = response.json().await?;
            debug!("Stock master data received: {} items", 
                data["t8436OutBlock"].as_array().map(|a| a.len()).unwrap_or(0));
        }
        
        Ok(())
    }

    async fn fetch_futures_master(&self) -> Result<()> {
        let client = reqwest::Client::new();
        let token = self.access_token.as_ref().ok_or(Error::Auth("No token".into()))?;
        
        let url = format!("{}/fuopt/market-data", Self::BASE_URL);
        let headers = json!({
            "content-type": "application/json; charset=utf-8",
            "authorization": format!("Bearer {}", token),
            "tr_cd": TransactionCodes::FUTURES_MASTER_LIST,
            "tr_cont": "N",
            "tr_cont_key": ""
        });
        
        let body = json!({"t2301InBlock": {"dummy": ""}});
        
        let response = client.post(&url)
            .json(&headers)
            .json(&body)
            .send()
            .await?;
        
        if response.status().is_success() {
            let data: Value = response.json().await?;
            debug!("Futures master data received");
        }
        
        Ok(())
    }

    async fn fetch_options_master(&self) -> Result<()> {
        let client = reqwest::Client::new();
        let token = self.access_token.as_ref().ok_or(Error::Auth("No token".into()))?;
        
        let url = format!("{}/fuopt/market-data", Self::BASE_URL);
        
        // Fetch both call and put options
        for option_type in ["2", "3"] {
            let headers = json!({
                "content-type": "application/json; charset=utf-8",
                "authorization": format!("Bearer {}", token),
                "tr_cd": TransactionCodes::OPTIONS_MASTER_LIST,
                "tr_cont": "N",
                "tr_cont_key": ""
            });
            
            let body = json!({"t2301InBlock": {"gubun": option_type}});
            
            let response = client.post(&url)
                .json(&headers)
                .json(&body)
                .send()
                .await?;
            
            if response.status().is_success() {
                let data: Value = response.json().await?;
                debug!("Options master data received for type {}", option_type);
            }
        }
        
        Ok(())
    }

    async fn fetch_etf_master(&self) -> Result<()> {
        let client = reqwest::Client::new();
        let token = self.access_token.as_ref().ok_or(Error::Auth("No token".into()))?;
        
        let url = format!("{}/stock/market-data", Self::BASE_URL);
        let headers = json!({
            "content-type": "application/json; charset=utf-8",
            "authorization": format!("Bearer {}", token),
            "tr_cd": TransactionCodes::ETF_MASTER_LIST,
            "tr_cont": "N",
            "tr_cont_key": ""
        });
        
        let body = json!({"t1404InBlock": {"gubun": "E"}});
        
        let response = client.post(&url)
            .json(&headers)
            .json(&body)
            .send()
            .await?;
        
        if response.status().is_success() {
            let data: Value = response.json().await?;
            debug!("ETF master data received");
        }
        
        Ok(())
    }

    async fn fetch_overseas_futures_master(&self) -> Result<()> {
        let client = reqwest::Client::new();
        let token = self.access_token.as_ref().ok_or(Error::Auth("No token".into()))?;
        
        let url = format!("{}/overseas-fuopt/market-data", Self::BASE_URL);
        let headers = json!({
            "content-type": "application/json; charset=utf-8",
            "authorization": format!("Bearer {}", token),
            "tr_cd": TransactionCodes::OVERSEAS_FUTURES_MASTER,
            "tr_cont": "N",
            "tr_cont_key": ""
        });
        
        let body = json!({"o3101InBlock": {"dummy": ""}});
        
        let response = client.post(&url)
            .json(&headers)
            .json(&body)
            .send()
            .await?;
        
        if response.status().is_success() {
            let data: Value = response.json().await?;
            debug!("Overseas futures master data received");
        }
        
        Ok(())
    }
}

#[async_trait]
impl FeederStock for LSEnhancedExchange {
    fn name(&self) -> &str {
        "LS_Enhanced"
    }

    async fn connect(&mut self) -> Result<()> {
        // Authenticate and get token
        self.authenticate().await?;
        
        // Establish WebSocket connection
        self.connect_websocket().await?;
        
        // Fetch master data for all configured asset types
        if !self.symbols_by_type.is_empty() {
            self.fetch_all_master_data().await?;
        }
        
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<()> {
        self.check_token_validity().await?;
        
        let mut ws_stream = self.ws_stream.take()
            .ok_or_else(|| Error::Connection("Not connected".into()))?;
        
        let subscriptions = self.create_enhanced_subscription()?;
        
        for subscription in subscriptions {
            ws_stream.send(Message::text(subscription)).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        self.ws_stream = Some(ws_stream);
        
        info!("Subscribed to {} symbols across {} asset types", 
            self.symbols_by_type.values().map(|v| v.len()).sum::<usize>(),
            self.symbols_by_type.len());
        
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        let mut ws_stream = self.ws_stream.take()
            .ok_or_else(|| Error::Connection("Not connected".into()))?;

        info!("Starting enhanced LS Securities WebSocket listener");
        
        {
            let mut stats = CONNECTION_STATS.write();
            let entry = stats.entry("LS_Enhanced".to_string()).or_insert_with(Default::default);
            entry.connected += 1;
            entry.total_connections += 1;
        }

        loop {
            tokio::select! {
                Some(msg) = ws_stream.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if let Ok(json_msg) = serde_json::from_str::<Value>(&text) {
                                if let Err(e) = self.process_enhanced_message(json_msg).await {
                                    warn!("Error processing message: {}", e);
                                }
                            }
                        },
                        Ok(Message::Ping(ping)) => {
                            ws_stream.send(Message::Pong(ping)).await?;
                        },
                        Ok(Message::Close(_)) => {
                            warn!("WebSocket connection closed by server");
                            break;
                        },
                        Err(e) => {
                            error!("WebSocket error: {}", e);
                            {
                                let mut stats = CONNECTION_STATS.write();
                                if let Some(entry) = stats.get_mut("LS_Enhanced") {
                                    entry.last_error = Some(e.to_string());
                                }
                            }
                            break;
                        },
                        _ => {}
                    }
                },
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    // Send ping to keep connection alive
                    ws_stream.send(Message::Ping(vec![].into())).await?;
                }
            }
        }

        {
            let mut stats = CONNECTION_STATS.write();
            if let Some(entry) = stats.get_mut("LS_Enhanced") {
                if entry.connected > 0 {
                    entry.connected -= 1;
                }
                entry.disconnected += 1;
            }
        }
        
        // Enhanced reconnection logic
        if self.reconnect_count < self.max_reconnect_attempts {
            self.reconnect_count += 1;
            let backoff = std::cmp::min(5 * (1 << self.reconnect_count), 300);
            warn!("Attempting reconnection {}/{} in {} seconds", 
                self.reconnect_count, self.max_reconnect_attempts, backoff);
            
            tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
            
            if let Err(e) = self.connect().await {
                error!("Failed to reconnect: {}", e);
            } else if let Err(e) = self.subscribe().await {
                error!("Failed to resubscribe: {}", e);
            } else {
                self.reconnect_count = 0;
                return self.start().await;
            }
        }

        Err(Error::Connection("Max reconnection attempts reached".into()))
    }
}