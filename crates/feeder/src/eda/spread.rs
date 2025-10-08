use chrono::Utc;

use std::collections::HashMap;
use crate::core::{TradeData, OrderBookData, TRADES, ORDERBOOKS, SymbolMapper};
use crate::storage::{FileStorage, SpreadData};



pub struct MarketDataComparator {
    symbol_mapper: SymbolMapper,
}


impl MarketDataComparator {
    pub fn new(symbol_mapper: SymbolMapper) -> Self {

        Self { 
            symbol_mapper,
        }
    }


    // /// Returns the global notifier used for triggering comparisons.
    // pub fn get_notifier(&self) -> Arc<Notify> {
    //     COMPARE_NOTIFY.clone()
    // }

    pub fn compare_trades(&self, symbol: &str) {
        tracing::debug!("Comparing trades for symbol: {}", symbol);

        let trades = TRADES.read();
        tracing::debug!("Number of trades in storage: {}", trades.len());

        // Get latest trade per exchange by timestamp
        let mut latest_trades: HashMap<String, &TradeData> = HashMap::new();
        
        for trade in trades.iter().filter(|t| t.symbol == symbol) {
            match latest_trades.get(&trade.exchange) {
                Some(existing) if trade.timestamp > existing.timestamp => {
                    latest_trades.insert(trade.exchange.clone(), trade);
                }
                None => {
                    latest_trades.insert(trade.exchange.clone(), trade);
                }
                _ => {}
            }
        }
        tracing::debug!("Number of exchanges with trades: {}", latest_trades.len());

        if latest_trades.len() >= 2 {
            let trades_vec: Vec<_> = latest_trades.iter().collect();
            
            // Compare each pair of exchanges
            for i in 0..trades_vec.len() {
                for j in i+1..trades_vec.len() {
                    let (exchange1, trade1) = trades_vec[i];
                    let (exchange2, trade2) = trades_vec[j];

                    // Convert scaled i64 to f64 for comparison
                    let price1_f64 = trade1.get_price_f64();
                    let price2_f64 = trade2.get_price_f64();

                    let spread = (price1_f64 - price2_f64).abs();
                    let spread_percent = (spread / price1_f64) * 100.0;

                    let spread_data = SpreadData {
                        timestamp: Utc::now().timestamp(),
                        symbol: symbol.to_string(),
                        exchange1: exchange1.clone(),
                        exchange2: exchange2.clone(),
                        price1: price1_f64,
                        price2: price2_f64,
                        spread,
                        spread_percent,
                    };

                    if let Err(e) = FileStorage::store_spread(&spread_data) {
                        tracing::error!("Failed to store spread data: {}", e);
                    }

                    // Log spread data
                    tracing::info!("Trade spread for {} between {} and {}: {:.2}%",
                        symbol, exchange1, exchange2, spread_percent);
                }
            }
        }
    }

    pub fn compare_orderbooks(&self, symbol: &str) {
        let orderbooks = ORDERBOOKS.read();
        
        // Get latest orderbook per exchange by timestamp
        let mut latest_orderbooks: HashMap<String, &OrderBookData> = HashMap::new();
        
        for ob in orderbooks.iter().filter(|ob| ob.symbol == symbol) {
            match latest_orderbooks.get(&ob.exchange) {
                Some(existing) if ob.timestamp > existing.timestamp => {
                    latest_orderbooks.insert(ob.exchange.clone(), ob);
                }
                None => {
                    latest_orderbooks.insert(ob.exchange.clone(), ob);
                }
                _ => {}
            }
        }

        if latest_orderbooks.len() >= 2 {
            tracing::debug!("Latest orderbook data for {} at {}", symbol, Utc::now());
            for (exchange, ob) in &latest_orderbooks {
                let best_bid_i64 = ob.bids.first().map_or(0, |&(price, _)| price);
                let best_ask_i64 = ob.asks.first().map_or(0, |&(price, _)| price);
                let best_bid = ob.unscale_price(best_bid_i64);
                let best_ask = ob.unscale_price(best_ask_i64);
                tracing::debug!("  {}: Bid {} / Ask {} (timestamp: {})",
                    exchange,
                    best_bid,
                    best_ask,
                    ob.timestamp
                );
            }
        }
    }

    // fn get_common_symbol(&self, exchange: &str, exchange_symbol: &str) -> Option<String> {
    //     self.symbol_mapping.exchanges.get(exchange)
    //         .and_then(|exchange_map| exchange_map.get(exchange_symbol))
    //         .map(|s| s.to_string())
    // }

    
    // pub fn update_trade(&self, mut trade: TradeData) {
    //     println!("Received trade update: {} {} @ {}", trade.exchange, trade.symbol, trade.price);  // Debug print

    //     let common_symbol = if let Some(symbol) = self.get_common_symbol(&trade.exchange, &trade.symbol) {
    //         symbol
    //     } else {
    //         eprintln!("No mapping found for {}/{}", trade.exchange, trade.symbol);
    //         return;
    //     };

    //     trade.symbol = common_symbol.clone();  // Update to common symbol
    //     println!("Mapped to common symbol: {}", common_symbol);  // Debug print

    //     let mut trades = TRADES.write();
        
    //     // // Remove old data for this exchange and symbol
    //     // trades.retain(|t| 
    //     //     !(t.exchange == trade.exchange && 
    //     //       t.symbol == trade.symbol) ||
    //     //     Utc::now().timestamp() - t.timestamp < 3600  // Keep last hour
    //     // );
        
    //     trades.push(trade);

    //     // Compare if we have multiple exchanges for this symbol
    //     let exchange_count = trades.iter()
    //         .filter(|t| t.symbol == common_symbol)
    //         .map(|t| &t.exchange)
    //         .collect::<std::collections::HashSet<_>>()
    //         .len();

    //     if exchange_count >= 2 {
    //         self.compare_trades(&common_symbol);
    //     }
    // }

    // pub fn update_orderbook(&self, mut orderbook: OrderBookData) {
    //     let common_symbol = if let Some(symbol) = self.get_common_symbol(&orderbook.exchange, &orderbook.symbol) {
    //         symbol
    //     } else {
    //         eprintln!("No mapping found for {}/{}", orderbook.exchange, orderbook.symbol);
    //         return;
    //     };

    //     orderbook.symbol = common_symbol.clone();

    //     let mut orderbooks = ORDERBOOKS.write();
    //     orderbooks.push(orderbook);        

    //     // Get exchange count before update
    //     let exchange_count = orderbooks.iter()
    //         .filter(|ob| ob.symbol == common_symbol)
    //         .map(|ob| &ob.exchange)
    //         .collect::<std::collections::HashSet<_>>()
    //         .len();

    //     // // Update or add new orderbook
    //     // if let Some(existing) = orderbooks.iter_mut()
    //     //     .find(|ob| ob.exchange == orderbook.exchange && ob.symbol == orderbook.symbol) 
    //     // {
    //     //     if orderbook.timestamp > existing.timestamp {
    //     //         *existing = orderbook;
    //     //     }
    //     // } else {
    //     //     orderbooks.push(orderbook);
    //     // }

    //     // Compare if we have multiple exchanges
    //     if exchange_count >= 1 {  // We already have at least one other exchange
    //         self.compare_orderbooks(&common_symbol);
    //     }
    // }


    // // Optional: Add cleanup method
    // fn cleanup_old_data() {
    //     let current_time = chrono::Utc::now().timestamp();
        
    //     let mut trades = TRADES.write();
    //     trades.retain(|t| current_time - t.timestamp < 3600); // Keep last hour
        
    //     let mut orderbooks = ORDERBOOKS.write();
    //     orderbooks.retain(|o| current_time - o.timestamp < 3600);
    // }



}

