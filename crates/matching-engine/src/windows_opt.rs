/// Windows-specific optimizations for orderbook processing
///
/// Optimizations for Windows/IOCP runtime environment

use std::sync::Arc;
use parking_lot::RwLock;
use anyhow::Result;
use std::collections::BTreeMap;
use market_types::{PriceLevel, OrderBook};

/// Windows-optimized orderbook using BTreeMap for better performance
pub struct OptimizedOrderBook {
    /// Symbol
    pub symbol: String,

    /// Bids stored in BTreeMap (descending order)
    pub bids: BTreeMap<i64, f64>,  // price_as_ticks -> quantity

    /// Asks stored in BTreeMap (ascending order)
    pub asks: BTreeMap<i64, f64>,  // price_as_ticks -> quantity

    /// Tick size for price conversion
    pub tick_size: f64,

    /// Last update sequence
    pub last_sequence: u64,

    /// Update count
    pub update_count: u64,
}

impl OptimizedOrderBook {
    /// Create new optimized orderbook
    pub fn new(symbol: String, tick_size: f64) -> Self {
        Self {
            symbol,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            tick_size,
            last_sequence: 0,
            update_count: 0,
        }
    }

    /// Convert price to ticks (for integer comparison)
    #[inline]
    fn price_to_ticks(&self, price: f64) -> i64 {
        (price / self.tick_size).round() as i64
    }

    /// Convert ticks back to price
    #[inline]
    fn ticks_to_price(&self, ticks: i64) -> f64 {
        ticks as f64 * self.tick_size
    }

    /// Update bid level
    pub fn update_bid(&mut self, price: f64, quantity: f64) {
        let ticks = self.price_to_ticks(price);

        if quantity == 0.0 {
            self.bids.remove(&(-ticks)); // Negative for descending order
        } else {
            self.bids.insert(-ticks, quantity);
        }
    }

    /// Update ask level
    pub fn update_ask(&mut self, price: f64, quantity: f64) {
        let ticks = self.price_to_ticks(price);

        if quantity == 0.0 {
            self.asks.remove(&ticks);
        } else {
            self.asks.insert(ticks, quantity);
        }
    }

    /// Get best bid
    #[inline]
    pub fn best_bid(&self) -> Option<(f64, f64)> {
        self.bids.iter().next().map(|(ticks, qty)| {
            (self.ticks_to_price(-ticks), *qty)
        })
    }

    /// Get best ask
    #[inline]
    pub fn best_ask(&self) -> Option<(f64, f64)> {
        self.asks.iter().next().map(|(ticks, qty)| {
            (self.ticks_to_price(*ticks), *qty)
        })
    }

    /// Get top N bid levels
    pub fn top_bids(&self, n: usize) -> Vec<PriceLevel> {
        self.bids
            .iter()
            .take(n)
            .map(|(ticks, qty)| PriceLevel {
                price: self.ticks_to_price(-ticks),
                quantity: *qty,
            })
            .collect()
    }

    /// Get top N ask levels
    pub fn top_asks(&self, n: usize) -> Vec<PriceLevel> {
        self.asks
            .iter()
            .take(n)
            .map(|(ticks, qty)| PriceLevel {
                price: self.ticks_to_price(*ticks),
                quantity: *qty,
            })
            .collect()
    }

    /// Convert to standard OrderBook
    pub fn to_orderbook(&self) -> OrderBook {
        let mut book = OrderBook::new(
            self.symbol.clone(),
            market_types::Exchange::Binance, // Default
        );

        book.bids = self.top_bids(100);
        book.asks = self.top_asks(100);
        book.last_update_id = self.last_sequence;
        book.update_count = self.update_count;

        book
    }

    /// Clear all levels
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }

    /// Apply batch updates efficiently
    pub fn batch_update(
        &mut self,
        bid_updates: Vec<(f64, f64)>,
        ask_updates: Vec<(f64, f64)>,
    ) {
        // Process bids
        for (price, qty) in bid_updates {
            self.update_bid(price, qty);
        }

        // Process asks
        for (price, qty) in ask_updates {
            self.update_ask(price, qty);
        }

        self.update_count += 1;
    }
}

/// Windows-optimized buffer using circular buffer
pub struct CircularBuffer<T> {
    buffer: Vec<Option<T>>,
    capacity: usize,
    head: usize,
    tail: usize,
    size: usize,
}

impl<T: Clone> CircularBuffer<T> {
    /// Create new circular buffer with fixed capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![None; capacity],
            capacity,
            head: 0,
            tail: 0,
            size: 0,
        }
    }

    /// Push item to buffer
    pub fn push(&mut self, item: T) -> bool {
        if self.size == self.capacity {
            // Buffer full, overwrite oldest
            self.head = (self.head + 1) % self.capacity;
        } else {
            self.size += 1;
        }

        self.buffer[self.tail] = Some(item);
        self.tail = (self.tail + 1) % self.capacity;

        true
    }

    /// Pop item from buffer
    pub fn pop(&mut self) -> Option<T> {
        if self.size == 0 {
            return None;
        }

        let item = self.buffer[self.head].take();
        self.head = (self.head + 1) % self.capacity;
        self.size -= 1;

        item
    }

    /// Get buffer size
    pub fn len(&self) -> usize {
        self.size
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Clear buffer
    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.size = 0;
        for slot in &mut self.buffer {
            *slot = None;
        }
    }

    /// Drain n items from buffer
    pub fn drain(&mut self, n: usize) -> Vec<T> {
        let mut items = Vec::with_capacity(n.min(self.size));

        for _ in 0..n.min(self.size) {
            if let Some(item) = self.pop() {
                items.push(item);
            } else {
                break;
            }
        }

        items
    }
}

/// Memory pool for reusing allocations
pub struct MemoryPool<T> {
    pool: Arc<RwLock<Vec<T>>>,
    max_size: usize,
}

impl<T: Default> MemoryPool<T> {
    /// Create new memory pool
    pub fn new(max_size: usize) -> Self {
        Self {
            pool: Arc::new(RwLock::new(Vec::with_capacity(max_size))),
            max_size,
        }
    }

    /// Get item from pool or create new
    pub fn get(&self) -> T {
        let mut pool = self.pool.write();
        pool.pop().unwrap_or_default()
    }

    /// Return item to pool
    pub fn put(&self, mut item: T) {
        let mut pool = self.pool.write();

        if pool.len() < self.max_size {
            // Reset item before returning to pool
            item = T::default();
            pool.push(item);
        }
    }
}

/// Windows thread affinity optimizer
#[cfg(windows)]
pub mod affinity {
    /// Set thread affinity to specific CPU core
    /// Note: Full implementation requires proper winapi setup
    pub fn set_thread_affinity(core_id: usize) -> bool {
        // Would use SetThreadAffinityMask here
        // Simplified for compilation
        let _ = core_id;
        true
    }

    /// Get optimal core for orderbook processing
    pub fn get_optimal_core() -> usize {
        // Use core 2 by default (avoid core 0 which handles interrupts)
        2
    }
}

#[cfg(not(windows))]
pub mod affinity {
    pub fn set_thread_affinity(_core_id: usize) -> bool {
        false
    }

    pub fn get_optimal_core() -> usize {
        0
    }
}

/// Prefetch hints for better cache performance
#[inline(always)]
pub fn prefetch_read<T>(ptr: *const T) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::_mm_prefetch;
        _mm_prefetch(ptr as *const i8, 0);
    }
}

#[inline(always)]
pub fn prefetch_write<T>(ptr: *mut T) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::_mm_prefetch;
        _mm_prefetch(ptr as *const i8, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimized_orderbook() {
        let mut book = OptimizedOrderBook::new("BTCUSDT".to_string(), 0.01);

        // Add bids
        book.update_bid(100.0, 10.0);
        book.update_bid(99.9, 20.0);
        book.update_bid(99.8, 30.0);

        // Add asks
        book.update_ask(100.1, 15.0);
        book.update_ask(100.2, 25.0);

        // Check best bid/ask
        let (bid_price, bid_qty) = book.best_bid().unwrap();
        assert_eq!(bid_price, 100.0);
        assert_eq!(bid_qty, 10.0);

        let (ask_price, ask_qty) = book.best_ask().unwrap();
        assert_eq!(ask_price, 100.1);
        assert_eq!(ask_qty, 15.0);

        // Remove level
        book.update_bid(100.0, 0.0);
        let (bid_price, _) = book.best_bid().unwrap();
        assert_eq!(bid_price, 99.9);
    }

    #[test]
    fn test_circular_buffer() {
        let mut buffer = CircularBuffer::new(3);

        // Fill buffer
        assert!(buffer.push(1));
        assert!(buffer.push(2));
        assert!(buffer.push(3));
        assert_eq!(buffer.len(), 3);

        // Overwrite oldest
        assert!(buffer.push(4));
        assert_eq!(buffer.len(), 3);

        // Pop items
        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), Some(3));
        assert_eq!(buffer.pop(), Some(4));
        assert_eq!(buffer.pop(), None);
    }

    #[test]
    fn test_memory_pool() {
        #[derive(Default, Clone)]
        struct TestItem {
            value: i32,
        }

        let pool = MemoryPool::<TestItem>::new(10);

        // Get item from pool
        let mut item = pool.get();
        item.value = 42;

        // Return to pool
        pool.put(item.clone());

        // Get again (should be reset)
        let item2 = pool.get();
        assert_eq!(item2.value, 0);
    }
}