/// NBBO (National Best Bid and Offer) Module
/// Computes cross-exchange best bid/ask prices for optimal trading
/// Based on Julia implementation from PricingPlatform/nbbo_consolidated_view.jl

use crate::types::{Exchange, OrderBook, PriceLevel, Timestamp};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use anyhow::{Result, bail};

/// NBBO data for a single symbol across all exchanges
#[derive(Debug, Clone)]
pub struct NBBO {
    /// Trading symbol (normalized)
    pub symbol: String,

    /// Best bid price across all exchanges
    pub best_bid: f64,

    /// Quantity at best bid
    pub best_bid_qty: f64,

    /// Exchange providing best bid
    pub best_bid_exchange: Exchange,

    /// Best ask price across all exchanges
    pub best_ask: f64,

    /// Quantity at best ask
    pub best_ask_qty: f64,

    /// Exchange providing best ask
    pub best_ask_exchange: Exchange,

    /// NBBO mid price (best_bid + best_ask) / 2
    pub nbbo_mid: f64,

    /// NBBO spread (best_ask - best_bid)
    pub nbbo_spread: f64,

    /// NBBO spread in basis points
    pub nbbo_spread_bps: f64,

    /// Number of exchanges contributing data
    pub num_exchanges: usize,

    /// Last update timestamp
    pub timestamp: Timestamp,

    /// Validation quality score (0.0 to 1.0)
    pub validation_rate: f64,
}

impl NBBO {
    /// Check if NBBO represents a crossed market (bid > ask)
    pub fn is_crossed(&self) -> bool {
        self.best_bid >= self.best_ask
    }

    /// Check if NBBO represents a locked market (bid == ask)
    pub fn is_locked(&self) -> bool {
        (self.best_bid - self.best_ask).abs() < f64::EPSILON
    }

    /// Check if NBBO is valid (has both bid and ask)
    pub fn is_valid(&self) -> bool {
        self.best_bid > 0.0 && self.best_ask > 0.0 && self.best_ask < f64::MAX
    }

    /// Calculate potential arbitrage profit if crossed
    pub fn arbitrage_profit(&self) -> f64 {
        if self.is_crossed() {
            self.best_bid - self.best_ask
        } else {
            0.0
        }
    }
}

/// Exchange order book state
#[derive(Debug, Clone)]
struct ExchangeBookState {
    order_book: OrderBook,
    last_update: Timestamp,
}

/// NBBO Manager - maintains cross-exchange NBBO calculation
pub struct NBBOManager {
    /// Order books indexed by (symbol, exchange)
    order_books: Arc<DashMap<(String, Exchange), Arc<RwLock<ExchangeBookState>>>>,

    /// Cached NBBO values indexed by symbol
    nbbo_cache: Arc<DashMap<String, Arc<RwLock<NBBO>>>>,

    /// Statistics
    total_updates: Arc<RwLock<u64>>,
    nbbo_computations: Arc<RwLock<u64>>,
    crossed_markets: Arc<RwLock<u64>>,

    /// Configuration
    max_stale_ms: i64,
}

impl NBBOManager {
    /// Create new NBBO manager
    pub fn new(max_stale_ms: i64) -> Self {
        Self {
            order_books: Arc::new(DashMap::new()),
            nbbo_cache: Arc::new(DashMap::new()),
            total_updates: Arc::new(RwLock::new(0)),
            nbbo_computations: Arc::new(RwLock::new(0)),
            crossed_markets: Arc::new(RwLock::new(0)),
            max_stale_ms,
        }
    }

    /// Update order book for a specific exchange and recompute NBBO
    pub fn update_order_book(
        &self,
        symbol: String,
        exchange: Exchange,
        order_book: OrderBook,
        timestamp: Timestamp,
    ) -> Result<Arc<RwLock<NBBO>>> {
        *self.total_updates.write() += 1;

        // Validate order book
        if order_book.bids.is_empty() && order_book.asks.is_empty() {
            bail!("Empty order book");
        }

        // Update order book state
        let key = (symbol.clone(), exchange);
        let book_state = ExchangeBookState {
            order_book,
            last_update: timestamp,
        };

        self.order_books
            .entry(key)
            .or_insert_with(|| Arc::new(RwLock::new(book_state.clone())))
            .write()
            .clone_from(&book_state);

        // Recompute NBBO for this symbol
        self.compute_nbbo(&symbol, timestamp)
    }

    /// Compute NBBO across all exchanges for a symbol
    pub fn compute_nbbo(
        &self,
        symbol: &str,
        current_time: Timestamp,
    ) -> Result<Arc<RwLock<NBBO>>> {
        *self.nbbo_computations.write() += 1;

        let mut best_bid = 0.0;
        let mut best_bid_qty = 0.0;
        let mut best_bid_exchange = Exchange::Binance;

        let mut best_ask = f64::MAX;
        let mut best_ask_qty = 0.0;
        let mut best_ask_exchange = Exchange::Binance;

        let mut num_exchanges = 0;
        let mut total_valid = 0;
        let mut total_checked = 0;

        // Iterate through all exchanges for this symbol
        for entry in self.order_books.iter() {
            let (sym, exchange) = entry.key();

            if sym != symbol {
                continue;
            }

            total_checked += 1;

            let book_state = entry.value().read();

            // Check if stale
            let age_ms = (current_time - book_state.last_update) / 1000;
            if age_ms > self.max_stale_ms {
                continue;
            }

            total_valid += 1;
            num_exchanges += 1;

            // Check best bid
            if let Some(bid_level) = book_state.order_book.bids.first() {
                if bid_level.price > best_bid {
                    best_bid = bid_level.price;
                    best_bid_qty = bid_level.quantity;
                    best_bid_exchange = *exchange;
                }
            }

            // Check best ask
            if let Some(ask_level) = book_state.order_book.asks.first() {
                if ask_level.price < best_ask {
                    best_ask = ask_level.price;
                    best_ask_qty = ask_level.quantity;
                    best_ask_exchange = *exchange;
                }
            }
        }

        // Validate we have both bid and ask
        if best_bid <= 0.0 || best_ask >= f64::MAX {
            bail!("Incomplete NBBO: no valid bid or ask");
        }

        // Calculate derived values
        let nbbo_mid = (best_bid + best_ask) / 2.0;
        let nbbo_spread = best_ask - best_bid;
        let nbbo_spread_bps = (nbbo_spread / nbbo_mid) * 10000.0;

        // Track crossed markets
        if nbbo_spread < 0.0 {
            *self.crossed_markets.write() += 1;
        }

        // Calculate validation rate
        let validation_rate = if total_checked > 0 {
            total_valid as f64 / total_checked as f64
        } else {
            0.0
        };

        // Create NBBO
        let nbbo = NBBO {
            symbol: symbol.to_string(),
            best_bid,
            best_bid_qty,
            best_bid_exchange,
            best_ask,
            best_ask_qty,
            best_ask_exchange,
            nbbo_mid,
            nbbo_spread,
            nbbo_spread_bps,
            num_exchanges,
            timestamp: current_time,
            validation_rate,
        };

        // Update cache
        let nbbo_arc = Arc::new(RwLock::new(nbbo));
        self.nbbo_cache
            .entry(symbol.to_string())
            .or_insert_with(|| nbbo_arc.clone())
            .write()
            .clone_from(&*nbbo_arc.read());

        Ok(nbbo_arc)
    }

    /// Get cached NBBO for a symbol
    pub fn get_nbbo(&self, symbol: &str) -> Option<NBBO> {
        self.nbbo_cache
            .get(symbol)
            .map(|nbbo_ref| nbbo_ref.read().clone())
    }

    /// Get all NBBO values
    pub fn get_all_nbbo(&self) -> Vec<NBBO> {
        self.nbbo_cache
            .iter()
            .map(|entry| entry.value().read().clone())
            .collect()
    }

    /// Get NBBO statistics
    pub fn get_statistics(&self) -> NBBOStatistics {
        let all_nbbo = self.get_all_nbbo();

        if all_nbbo.is_empty() {
            return NBBOStatistics::default();
        }

        let spreads_bps: Vec<f64> = all_nbbo.iter().map(|n| n.nbbo_spread_bps).collect();

        let avg_spread_bps = spreads_bps.iter().sum::<f64>() / spreads_bps.len() as f64;
        let min_spread_bps = spreads_bps.iter().cloned().fold(f64::MAX, f64::min);
        let max_spread_bps = spreads_bps.iter().cloned().fold(f64::MIN, f64::max);

        // Count exchange participation
        let mut bid_exchange_counts = std::collections::HashMap::new();
        let mut ask_exchange_counts = std::collections::HashMap::new();

        for nbbo in &all_nbbo {
            *bid_exchange_counts.entry(nbbo.best_bid_exchange).or_insert(0) += 1;
            *ask_exchange_counts.entry(nbbo.best_ask_exchange).or_insert(0) += 1;
        }

        let locked_markets = all_nbbo.iter().filter(|n| n.is_locked()).count();
        let crossed_markets = all_nbbo.iter().filter(|n| n.is_crossed()).count();

        NBBOStatistics {
            total_symbols: all_nbbo.len(),
            avg_spread_bps,
            min_spread_bps,
            max_spread_bps,
            bid_exchange_counts,
            ask_exchange_counts,
            locked_markets,
            crossed_markets,
            total_updates: *self.total_updates.read(),
            nbbo_computations: *self.nbbo_computations.read(),
        }
    }

    /// Clean up stale order books
    pub fn cleanup_stale(&self, current_time: Timestamp) {
        let mut to_remove = Vec::new();

        for entry in self.order_books.iter() {
            let book_state = entry.value().read();
            let age_ms = (current_time - book_state.last_update) / 1000;

            if age_ms > self.max_stale_ms {
                to_remove.push(entry.key().clone());
            }
        }

        for key in to_remove {
            self.order_books.remove(&key);
        }
    }
}

/// NBBO statistics
#[derive(Debug, Clone, Default)]
pub struct NBBOStatistics {
    pub total_symbols: usize,
    pub avg_spread_bps: f64,
    pub min_spread_bps: f64,
    pub max_spread_bps: f64,
    pub bid_exchange_counts: std::collections::HashMap<Exchange, usize>,
    pub ask_exchange_counts: std::collections::HashMap<Exchange, usize>,
    pub locked_markets: usize,
    pub crossed_markets: usize,
    pub total_updates: u64,
    pub nbbo_computations: u64,
}

impl std::fmt::Display for NBBOStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "NBBO Statistics:")?;
        writeln!(f, "  Total Symbols: {}", self.total_symbols)?;
        writeln!(f, "  Average Spread: {:.2} bps", self.avg_spread_bps)?;
        writeln!(f, "  Tightest Spread: {:.2} bps", self.min_spread_bps)?;
        writeln!(f, "  Widest Spread: {:.2} bps", self.max_spread_bps)?;
        writeln!(f, "  Locked Markets: {}", self.locked_markets)?;
        writeln!(f, "  Crossed Markets: {}", self.crossed_markets)?;
        writeln!(f, "  Total Updates: {}", self.total_updates)?;
        writeln!(f, "  NBBO Computations: {}", self.nbbo_computations)?;
        writeln!(f, "\nBest Bid Providers:")?;
        for (exchange, count) in &self.bid_exchange_counts {
            let pct = (*count as f64 / self.total_symbols as f64) * 100.0;
            writeln!(f, "  {:?}: {} ({:.1}%)", exchange, count, pct)?;
        }
        writeln!(f, "\nBest Ask Providers:")?;
        for (exchange, count) in &self.ask_exchange_counts {
            let pct = (*count as f64 / self.total_symbols as f64) * 100.0;
            writeln!(f, "  {:?}: {} ({:.1}%)", exchange, count, pct)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_order_book(bid_price: f64, ask_price: f64) -> OrderBook {
        OrderBook {
            symbol: "BTC-USD".to_string(),
            exchange: Exchange::Binance,
            timestamp: 0,
            bids: vec![PriceLevel { price: bid_price, quantity: 1.0 }],
            asks: vec![PriceLevel { price: ask_price, quantity: 1.0 }],
        }
    }

    #[test]
    fn test_nbbo_calculation() {
        let manager = NBBOManager::new(60000); // 60 second max stale

        // Add order books from different exchanges
        let book1 = create_test_order_book(100.0, 101.0);
        let book2 = create_test_order_book(100.5, 100.8);
        let book3 = create_test_order_book(99.8, 101.2);

        let timestamp = chrono::Utc::now().timestamp_micros();

        manager.update_order_book("BTC-USD".to_string(), Exchange::Binance, book1, timestamp).unwrap();
        manager.update_order_book("BTC-USD".to_string(), Exchange::OKX, book2, timestamp).unwrap();
        manager.update_order_book("BTC-USD".to_string(), Exchange::Bybit, book3, timestamp).unwrap();

        let nbbo = manager.get_nbbo("BTC-USD").unwrap();

        // Best bid should be 100.5 from OKX
        assert_eq!(nbbo.best_bid, 100.5);
        assert_eq!(nbbo.best_bid_exchange, Exchange::OKX);

        // Best ask should be 100.8 from OKX
        assert_eq!(nbbo.best_ask, 100.8);
        assert_eq!(nbbo.best_ask_exchange, Exchange::OKX);

        // Check derived values
        assert_eq!(nbbo.nbbo_mid, 100.65);
        assert_eq!(nbbo.nbbo_spread, 0.3);
        assert!(nbbo.nbbo_spread_bps > 0.0);
    }

    #[test]
    fn test_crossed_market_detection() {
        let nbbo = NBBO {
            symbol: "TEST".to_string(),
            best_bid: 101.0,
            best_bid_qty: 1.0,
            best_bid_exchange: Exchange::Binance,
            best_ask: 100.0,
            best_ask_qty: 1.0,
            best_ask_exchange: Exchange::OKX,
            nbbo_mid: 100.5,
            nbbo_spread: -1.0,
            nbbo_spread_bps: -100.0,
            num_exchanges: 2,
            timestamp: 0,
            validation_rate: 1.0,
        };

        assert!(nbbo.is_crossed());
        assert_eq!(nbbo.arbitrage_profit(), 1.0);
    }
}
