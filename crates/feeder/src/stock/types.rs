use chrono::{DateTime, Utc};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stock {
    pub code: String,         // Stock code without 'A' prefix
    pub name: String,         // Stock name
    pub market_cap: f64,      // To track top 100
    pub sector: String,       // Sector information
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Future {
    pub code: String,         // Future code
    pub expiry_date: DateTime<Utc>,
    pub underlying_stock: String,  // Reference to stock code
}

#[derive(Debug, Clone, Serialize, Hash, Eq, PartialEq, Deserialize)]
pub enum ContractMonth {
    Near,
    Next,
    Far,
}

#[derive(Debug, Clone)]
pub struct StockFutureMapping {
    pub stock: Stock,
    pub futures: HashMap<ContractMonth, Future>,
}

// Helper struct for organizing subscriptions
#[derive(Debug)]
pub struct SubscriptionGroup {
    pub stock_codes: Vec<String>,          // For stock price/trade subscriptions
    pub future_codes: Vec<(String, ContractMonth)>, // For future price/trade subscriptions
}
