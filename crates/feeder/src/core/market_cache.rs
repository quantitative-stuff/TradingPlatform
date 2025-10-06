/// Symbol-centric lock-free cache for HFT market data
/// Based on atomic operations to avoid context switching
/// Architecture: https://docs.google.com/document/d/UDP_Multicast_architecture

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

/// Cache-line aligned symbol slot (64 bytes)
/// Each slot stores the latest market data for one (exchange, symbol) pair
#[repr(align(64))]
#[derive(Debug)]
pub struct SymbolSlot {
    /// Sequence number for consistency (even = stable, odd = updating)
    seq: AtomicU64,

    /// Best bid price (scaled by 10^8)
    best_bid: AtomicI64,

    /// Best ask price (scaled by 10^8)
    best_ask: AtomicI64,

    /// Last trade price (scaled by 10^8)
    last_trade: AtomicI64,

    /// Exchange timestamp (nanoseconds)
    exchange_ts: AtomicU64,

    /// Local receive timestamp (nanoseconds)
    local_ts: AtomicU64,

    /// Bid quantity at best level (scaled by 10^8)
    bid_qty: AtomicI64,

    /// Ask quantity at best level (scaled by 10^8)
    ask_qty: AtomicI64,
}

impl SymbolSlot {
    /// Create new empty slot
    pub fn new() -> Self {
        SymbolSlot {
            seq: AtomicU64::new(0),
            best_bid: AtomicI64::new(0),
            best_ask: AtomicI64::new(0),
            last_trade: AtomicI64::new(0),
            exchange_ts: AtomicU64::new(0),
            local_ts: AtomicU64::new(0),
            bid_qty: AtomicI64::new(0),
            ask_qty: AtomicI64::new(0),
        }
    }

    /// Fast update without sequence number (lowest latency)
    /// Use when single-field atomicity is sufficient
    pub fn update_bid_fast(&self, price: i64, qty: i64, exch_ts: u64, local_ts: u64) {
        self.best_bid.store(price, Ordering::Release);
        self.bid_qty.store(qty, Ordering::Release);
        self.exchange_ts.store(exch_ts, Ordering::Release);
        self.local_ts.store(local_ts, Ordering::Release);
    }

    pub fn update_ask_fast(&self, price: i64, qty: i64, exch_ts: u64, local_ts: u64) {
        self.best_ask.store(price, Ordering::Release);
        self.ask_qty.store(qty, Ordering::Release);
        self.exchange_ts.store(exch_ts, Ordering::Release);
        self.local_ts.store(local_ts, Ordering::Release);
    }

    pub fn update_trade_fast(&self, price: i64, exch_ts: u64, local_ts: u64) {
        self.last_trade.store(price, Ordering::Release);
        self.exchange_ts.store(exch_ts, Ordering::Release);
        self.local_ts.store(local_ts, Ordering::Release);
    }

    /// Update with sequence number for multi-field consistency
    /// Use when you need atomic snapshot of multiple fields
    pub fn update_orderbook_with_seq(
        &self,
        bid: i64,
        ask: i64,
        bid_qty: i64,
        ask_qty: i64,
        exch_ts: u64,
        local_ts: u64,
    ) {
        // Mark updating (odd sequence number)
        let seq = self.seq.load(Ordering::Relaxed).wrapping_add(1);
        self.seq.store(seq | 1, Ordering::Release);

        // Write all fields
        self.best_bid.store(bid, Ordering::Relaxed);
        self.best_ask.store(ask, Ordering::Relaxed);
        self.bid_qty.store(bid_qty, Ordering::Relaxed);
        self.ask_qty.store(ask_qty, Ordering::Relaxed);
        self.exchange_ts.store(exch_ts, Ordering::Relaxed);
        self.local_ts.store(local_ts, Ordering::Relaxed);

        // Mark stable (even sequence number)
        self.seq.store(seq.wrapping_add(1) & !1, Ordering::Release);
    }

    /// Read consistent snapshot with sequence validation
    /// Retries if caught mid-update
    pub fn read_snapshot(&self) -> MarketSnapshot {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);

            // If odd, writer is in progress - can still try but may be inconsistent
            let bid = self.best_bid.load(Ordering::Relaxed);
            let ask = self.best_ask.load(Ordering::Relaxed);
            let bid_qty = self.bid_qty.load(Ordering::Relaxed);
            let ask_qty = self.ask_qty.load(Ordering::Relaxed);
            let trade = self.last_trade.load(Ordering::Relaxed);
            let exch_ts = self.exchange_ts.load(Ordering::Relaxed);
            let local_ts = self.local_ts.load(Ordering::Relaxed);

            let s2 = self.seq.load(Ordering::Acquire);

            // Valid if sequence unchanged and even
            if s1 == s2 && s1 % 2 == 0 {
                return MarketSnapshot {
                    best_bid: bid,
                    best_ask: ask,
                    bid_qty,
                    ask_qty,
                    last_trade: trade,
                    exchange_ts: exch_ts,
                    local_ts,
                };
            }

            // Retry if caught mid-update
            std::hint::spin_loop();
        }
    }

    /// Fast read without sequence validation (lowest latency)
    /// May read slightly inconsistent data across fields
    pub fn read_bid_fast(&self) -> (i64, i64) {
        let price = self.best_bid.load(Ordering::Acquire);
        let qty = self.bid_qty.load(Ordering::Acquire);
        (price, qty)
    }

    pub fn read_ask_fast(&self) -> (i64, i64) {
        let price = self.best_ask.load(Ordering::Acquire);
        let qty = self.ask_qty.load(Ordering::Acquire);
        (price, qty)
    }

    pub fn read_trade_fast(&self) -> i64 {
        self.last_trade.load(Ordering::Acquire)
    }

    pub fn read_spread_fast(&self) -> i64 {
        let ask = self.best_ask.load(Ordering::Acquire);
        let bid = self.best_bid.load(Ordering::Acquire);
        ask - bid
    }
}

/// Consistent market data snapshot
#[derive(Debug, Clone, Copy)]
pub struct MarketSnapshot {
    pub best_bid: i64,      // Scaled by 10^8
    pub best_ask: i64,      // Scaled by 10^8
    pub bid_qty: i64,       // Scaled by 10^8
    pub ask_qty: i64,       // Scaled by 10^8
    pub last_trade: i64,    // Scaled by 10^8
    pub exchange_ts: u64,   // Nanoseconds
    pub local_ts: u64,      // Nanoseconds
}

impl MarketSnapshot {
    /// Get bid price as f64
    pub fn bid_price(&self) -> f64 {
        self.best_bid as f64 / 100_000_000.0
    }

    /// Get ask price as f64
    pub fn ask_price(&self) -> f64 {
        self.best_ask as f64 / 100_000_000.0
    }

    /// Get trade price as f64
    pub fn trade_price(&self) -> f64 {
        self.last_trade as f64 / 100_000_000.0
    }

    /// Get spread in basis points
    pub fn spread_bps(&self) -> f64 {
        if self.best_bid == 0 {
            return 0.0;
        }
        let spread = self.best_ask - self.best_bid;
        (spread as f64 / self.best_bid as f64) * 10_000.0
    }

    /// Get mid price
    pub fn mid_price(&self) -> f64 {
        (self.best_bid + self.best_ask) as f64 / 2.0 / 100_000_000.0
    }

    /// Calculate latency from exchange to local (microseconds)
    pub fn latency_us(&self) -> u64 {
        if self.local_ts > self.exchange_ts {
            (self.local_ts - self.exchange_ts) / 1_000
        } else {
            0
        }
    }
}

/// Global market cache storing all symbols across exchanges
pub struct MarketCache {
    /// [exchange_id][symbol_id] -> SymbolSlot
    slots: Vec<Vec<SymbolSlot>>,

    /// Number of exchanges
    num_exchanges: usize,

    /// Number of symbols per exchange
    num_symbols: usize,
}

impl MarketCache {
    /// Create new cache with specified dimensions
    pub fn new(num_exchanges: usize, num_symbols: usize) -> Self {
        let mut slots = Vec::with_capacity(num_exchanges);

        for _ in 0..num_exchanges {
            let mut exchange_slots = Vec::with_capacity(num_symbols);
            for _ in 0..num_symbols {
                exchange_slots.push(SymbolSlot::new());
            }
            slots.push(exchange_slots);
        }

        MarketCache {
            slots,
            num_exchanges,
            num_symbols,
        }
    }

    /// Get slot for specific exchange and symbol
    #[inline]
    pub fn get_slot(&self, exchange_id: usize, symbol_id: usize) -> Option<&SymbolSlot> {
        self.slots.get(exchange_id)?.get(symbol_id)
    }

    /// Calculate cross-exchange spread
    /// Returns (spread_scaled, exchange1_bid, exchange2_ask)
    pub fn cross_exchange_spread(
        &self,
        exchange1_id: usize,
        exchange2_id: usize,
        symbol_id: usize,
    ) -> Option<(i64, i64, i64)> {
        let slot1 = self.get_slot(exchange1_id, symbol_id)?;
        let slot2 = self.get_slot(exchange2_id, symbol_id)?;

        let (bid1, _) = slot1.read_bid_fast();
        let (ask2, _) = slot2.read_ask_fast();

        Some((bid1 - ask2, bid1, ask2))
    }

    /// Calculate price ratio between two exchanges
    pub fn price_ratio(
        &self,
        exchange1_id: usize,
        exchange2_id: usize,
        symbol_id: usize,
    ) -> Option<f64> {
        let slot1 = self.get_slot(exchange1_id, symbol_id)?;
        let slot2 = self.get_slot(exchange2_id, symbol_id)?;

        let trade1 = slot1.read_trade_fast();
        let trade2 = slot2.read_trade_fast();

        if trade2 == 0 {
            return None;
        }

        Some(trade1 as f64 / trade2 as f64)
    }

    /// Get statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            num_exchanges: self.num_exchanges,
            num_symbols: self.num_symbols,
            total_slots: self.num_exchanges * self.num_symbols,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub num_exchanges: usize,
    pub num_symbols: usize,
    pub total_slots: usize,
}

// Thread-safe wrapper
pub type SharedMarketCache = Arc<MarketCache>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_slot_size() {
        // Should be 64 bytes (cache-line aligned)
        assert_eq!(std::mem::size_of::<SymbolSlot>(), 64);
    }

    #[test]
    fn test_fast_update_read() {
        let slot = SymbolSlot::new();

        // Update bid
        slot.update_bid_fast(
            5000000000000, // 50000.0 * 10^8
            100000000,     // 1.0 * 10^8
            1000000,
            1000100,
        );

        let (price, qty) = slot.read_bid_fast();
        assert_eq!(price, 5000000000000);
        assert_eq!(qty, 100000000);
    }

    #[test]
    fn test_sequence_consistency() {
        let slot = SymbolSlot::new();

        // Update with sequence
        slot.update_orderbook_with_seq(
            5000000000000,  // bid
            5001000000000,  // ask
            100000000,      // bid_qty
            200000000,      // ask_qty
            1000000,
            1000100,
        );

        let snapshot = slot.read_snapshot();
        assert_eq!(snapshot.best_bid, 5000000000000);
        assert_eq!(snapshot.best_ask, 5001000000000);
        assert_eq!(snapshot.bid_qty, 100000000);
        assert_eq!(snapshot.ask_qty, 200000000);
    }

    #[test]
    fn test_market_cache() {
        let cache = MarketCache::new(8, 100);

        let slot = cache.get_slot(0, 0).unwrap();
        slot.update_bid_fast(5000000000000, 100000000, 1000000, 1000100);

        let (price, qty) = cache.get_slot(0, 0).unwrap().read_bid_fast();
        assert_eq!(price, 5000000000000);
        assert_eq!(qty, 100000000);
    }

    #[test]
    fn test_cross_exchange_spread() {
        let cache = MarketCache::new(2, 1);

        // Exchange 0: bid = 50000
        cache.get_slot(0, 0).unwrap().update_bid_fast(5000000000000, 100000000, 1000000, 1000100);

        // Exchange 1: ask = 49999
        cache.get_slot(1, 0).unwrap().update_ask_fast(4999900000000, 100000000, 1000000, 1000100);

        let (spread, bid, ask) = cache.cross_exchange_spread(0, 1, 0).unwrap();
        assert_eq!(spread, 100000000); // Positive spread = arbitrage opportunity
        assert_eq!(bid, 5000000000000);
        assert_eq!(ask, 4999900000000);
    }

    #[test]
    fn test_snapshot_conversions() {
        let slot = SymbolSlot::new();
        slot.update_orderbook_with_seq(
            5000000000000,  // 50000.0
            5001000000000,  // 50010.0
            100000000,
            200000000,
            1000000,
            1010000,
        );

        let snapshot = slot.read_snapshot();
        assert_eq!(snapshot.bid_price(), 50000.0);
        assert_eq!(snapshot.ask_price(), 50010.0);
        assert_eq!(snapshot.mid_price(), 50005.0);
        assert_eq!(snapshot.latency_us(), 10); // 10 microseconds
    }
}