/// HFT-Style Lightweight OrderBook System
///
/// Professional-grade orderbook management following HFT firm practices:
/// - Single-threaded processing for all symbols
/// - Pre-allocated fixed-size structures
/// - Zero allocations in hot path
/// - Lock-free ring buffers
/// - Integer arithmetic only
/// - Cache-optimized memory layout

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;
use market_types::{Exchange, OrderBookUpdate, PriceLevel};
use tracing::{info, warn, debug};

/// Maximum symbols we can handle
const MAX_SYMBOLS: usize = 2048;

/// Maximum depth per orderbook (top 20 levels each side)
const MAX_DEPTH: usize = 20;

/// Ring buffer size for updates
const RING_BUFFER_SIZE: usize = 65536;

/// Cache line size for alignment
const CACHE_LINE_SIZE: usize = 64;

/// Fast price level with integer representation
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct FastPriceLevel {
    pub price_ticks: i64,  // Price as integer ticks
    pub quantity: i64,     // Quantity as integer
}

impl Default for FastPriceLevel {
    fn default() -> Self {
        Self {
            price_ticks: 0,
            quantity: 0,
        }
    }
}

/// Cache-aligned fast orderbook
#[repr(C, align(64))]
#[derive(Clone, Debug)]
struct FastOrderBook {
    // Hot data - frequently accessed
    bids: [FastPriceLevel; MAX_DEPTH],
    asks: [FastPriceLevel; MAX_DEPTH],
    bid_count: u8,
    ask_count: u8,
    symbol_id: u16,
    last_update_id: u64,

    // Cold data - rarely accessed
    total_bid_volume: i64,
    total_ask_volume: i64,
    last_update_time: u64,
    update_count: u64,

    // Padding to ensure cache alignment
    _padding: [u8; 14],
}

impl FastOrderBook {
    fn new(symbol_id: u16) -> Self {
        Self {
            bids: [FastPriceLevel::default(); MAX_DEPTH],
            asks: [FastPriceLevel::default(); MAX_DEPTH],
            bid_count: 0,
            ask_count: 0,
            symbol_id,
            last_update_id: 0,
            total_bid_volume: 0,
            total_ask_volume: 0,
            last_update_time: 0,
            update_count: 0,
            _padding: [0; 14],
        }
    }

    /// Apply update directly to orderbook (no allocations)
    #[inline(always)]
    fn apply_update(&mut self, update: &FastUpdate) {
        // Check sequence
        if update.update_id <= self.last_update_id {
            return; // Old update, skip
        }

        // Update bids
        for i in 0..update.bid_count as usize {
            let level = &update.bid_updates[i];
            if level.quantity == 0 {
                // Remove this price level
                self.remove_bid(level.price_ticks);
            } else {
                // Add or update level
                self.update_bid(level.price_ticks, level.quantity);
            }
        }

        // Update asks
        for i in 0..update.ask_count as usize {
            let level = &update.ask_updates[i];
            if level.quantity == 0 {
                // Remove this price level
                self.remove_ask(level.price_ticks);
            } else {
                // Add or update level
                self.update_ask(level.price_ticks, level.quantity);
            }
        }

        self.last_update_id = update.update_id;
        self.update_count += 1;
    }

    #[inline]
    fn update_bid(&mut self, price_ticks: i64, quantity: i64) {
        // Find position to insert (bids are sorted descending)
        let mut insert_pos = self.bid_count as usize;

        for i in 0..self.bid_count as usize {
            if self.bids[i].price_ticks == price_ticks {
                // Update existing level
                self.bids[i].quantity = quantity;
                return;
            }
            if self.bids[i].price_ticks < price_ticks {
                insert_pos = i;
                break;
            }
        }

        // Insert new level if we have room
        if insert_pos < MAX_DEPTH {
            // Shift levels down
            if self.bid_count as usize >= MAX_DEPTH {
                // Remove last level
                for i in (insert_pos + 1..MAX_DEPTH).rev() {
                    self.bids[i] = self.bids[i - 1];
                }
            } else {
                // Shift without removing
                for i in (insert_pos..self.bid_count as usize).rev() {
                    self.bids[i + 1] = self.bids[i];
                }
                self.bid_count += 1;
            }

            self.bids[insert_pos] = FastPriceLevel { price_ticks, quantity };
        }
    }

    #[inline]
    fn update_ask(&mut self, price_ticks: i64, quantity: i64) {
        // Find position to insert (asks are sorted ascending)
        let mut insert_pos = self.ask_count as usize;

        for i in 0..self.ask_count as usize {
            if self.asks[i].price_ticks == price_ticks {
                // Update existing level
                self.asks[i].quantity = quantity;
                return;
            }
            if self.asks[i].price_ticks > price_ticks {
                insert_pos = i;
                break;
            }
        }

        // Insert new level if we have room
        if insert_pos < MAX_DEPTH {
            // Shift levels down
            if self.ask_count as usize >= MAX_DEPTH {
                // Remove last level
                for i in (insert_pos + 1..MAX_DEPTH).rev() {
                    self.asks[i] = self.asks[i - 1];
                }
            } else {
                // Shift without removing
                for i in (insert_pos..self.ask_count as usize).rev() {
                    self.asks[i + 1] = self.asks[i];
                }
                self.ask_count += 1;
            }

            self.asks[insert_pos] = FastPriceLevel { price_ticks, quantity };
        }
    }

    #[inline]
    fn remove_bid(&mut self, price_ticks: i64) {
        for i in 0..self.bid_count as usize {
            if self.bids[i].price_ticks == price_ticks {
                // Shift remaining levels up
                for j in i..self.bid_count as usize - 1 {
                    self.bids[j] = self.bids[j + 1];
                }
                self.bid_count -= 1;
                break;
            }
        }
    }

    #[inline]
    fn remove_ask(&mut self, price_ticks: i64) {
        for i in 0..self.ask_count as usize {
            if self.asks[i].price_ticks == price_ticks {
                // Shift remaining levels up
                for j in i..self.ask_count as usize - 1 {
                    self.asks[j] = self.asks[j + 1];
                }
                self.ask_count -= 1;
                break;
            }
        }
    }
}

/// Fast update structure with fixed size
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FastUpdate {
    pub symbol_id: u16,
    pub update_id: u64,
    pub timestamp: u64,
    pub bid_updates: [FastPriceLevel; 10],  // Max 10 updates per side
    pub ask_updates: [FastPriceLevel; 10],
    pub bid_count: u8,
    pub ask_count: u8,
    pub is_snapshot: bool,
}

/// Lock-free SPSC ring buffer for updates
pub struct RingBuffer {
    buffer: Box<[std::cell::UnsafeCell<FastUpdate>; RING_BUFFER_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    cache_line_pad: [u8; CACHE_LINE_SIZE - 16],
}

unsafe impl Sync for RingBuffer {}
unsafe impl Send for RingBuffer {}

impl RingBuffer {
    fn new() -> Self {
        // Create array of UnsafeCell
        let mut v = Vec::with_capacity(RING_BUFFER_SIZE);
        for _ in 0..RING_BUFFER_SIZE {
            v.push(std::cell::UnsafeCell::new(unsafe { std::mem::zeroed() }));
        }
        let buffer: Box<[std::cell::UnsafeCell<FastUpdate>; RING_BUFFER_SIZE]> =
            v.into_boxed_slice().try_into().unwrap();

        Self {
            buffer,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            cache_line_pad: [0; CACHE_LINE_SIZE - 16],
        }
    }

    /// Push update to ring buffer (producer side)
    pub fn push(&self, update: FastUpdate) -> bool {
        let tail = self.tail.load(Ordering::Acquire);
        let next_tail = (tail + 1) % RING_BUFFER_SIZE;

        // Check if buffer is full
        if next_tail == self.head.load(Ordering::Acquire) {
            return false; // Buffer full
        }

        // Write update
        unsafe {
            *self.buffer[tail].get() = update;
        }

        self.tail.store(next_tail, Ordering::Release);
        true
    }

    /// Pop update from ring buffer (consumer side)
    fn pop(&self) -> Option<FastUpdate> {
        let head = self.head.load(Ordering::Acquire);

        // Check if buffer is empty
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }

        // Read update
        let update = unsafe {
            *self.buffer[head].get()
        };

        self.head.store((head + 1) % RING_BUFFER_SIZE, Ordering::Release);
        Some(update)
    }
}

/// Main HFT OrderBook Processor
pub struct HFTOrderBookProcessor {
    // All orderbooks in contiguous memory
    orderbooks: Box<[FastOrderBook; MAX_SYMBOLS]>,

    // Symbol name to ID mapping (built at startup)
    symbol_to_id: std::collections::HashMap<String, u16>,
    id_to_symbol: Vec<String>,

    // Ring buffers for each exchange
    binance_ring: Arc<RingBuffer>,
    bybit_ring: Arc<RingBuffer>,
    okx_ring: Arc<RingBuffer>,

    // Statistics
    total_updates: AtomicU64,
    updates_per_second: AtomicU64,

    // Tick sizes for price conversion
    tick_sizes: Vec<f64>,
}

impl HFTOrderBookProcessor {
    /// Create new processor
    pub fn new() -> Self {
        // Pre-allocate all orderbooks
        let mut orderbooks = Vec::with_capacity(MAX_SYMBOLS);
        for i in 0..MAX_SYMBOLS {
            orderbooks.push(FastOrderBook::new(i as u16));
        }
        let orderbooks: Box<[FastOrderBook; MAX_SYMBOLS]> =
            orderbooks.into_boxed_slice().try_into().unwrap();

        Self {
            orderbooks,
            symbol_to_id: std::collections::HashMap::new(),
            id_to_symbol: Vec::with_capacity(MAX_SYMBOLS),
            binance_ring: Arc::new(RingBuffer::new()),
            bybit_ring: Arc::new(RingBuffer::new()),
            okx_ring: Arc::new(RingBuffer::new()),
            total_updates: AtomicU64::new(0),
            updates_per_second: AtomicU64::new(0),
            tick_sizes: vec![0.01; MAX_SYMBOLS], // Default tick size
        }
    }

    /// Register a symbol and get its ID
    pub fn register_symbol(&mut self, symbol: &str, tick_size: f64) -> u16 {
        if let Some(&id) = self.symbol_to_id.get(symbol) {
            return id;
        }

        let id = self.id_to_symbol.len() as u16;
        self.symbol_to_id.insert(symbol.to_string(), id);
        self.id_to_symbol.push(symbol.to_string());
        self.tick_sizes[id as usize] = tick_size;

        info!("Registered symbol {} with ID {}", symbol, id);
        id
    }

    /// Convert OrderBookUpdate to FastUpdate
    pub fn convert_update(&self, update: &OrderBookUpdate) -> Option<FastUpdate> {
        let symbol_id = *self.symbol_to_id.get(&update.symbol)?;
        let tick_size = self.tick_sizes[symbol_id as usize];
        let tick_multiplier = 1.0 / tick_size;

        let mut fast_update = FastUpdate {
            symbol_id,
            update_id: update.update_id,
            timestamp: update.timestamp as u64,
            bid_updates: [FastPriceLevel::default(); 10],
            ask_updates: [FastPriceLevel::default(); 10],
            bid_count: 0,
            ask_count: 0,
            is_snapshot: update.is_snapshot,
        };

        // Convert bids (max 10)
        for (i, level) in update.bids.iter().take(10).enumerate() {
            fast_update.bid_updates[i] = FastPriceLevel {
                price_ticks: (level.price * tick_multiplier) as i64,
                quantity: (level.quantity * 1e8) as i64,
            };
            fast_update.bid_count += 1;
        }

        // Convert asks (max 10)
        for (i, level) in update.asks.iter().take(10).enumerate() {
            fast_update.ask_updates[i] = FastPriceLevel {
                price_ticks: (level.price * tick_multiplier) as i64,
                quantity: (level.quantity * 1e8) as i64,
            };
            fast_update.ask_count += 1;
        }

        Some(fast_update)
    }

    /// Get ring buffer for exchange
    pub fn get_ring_buffer(&self, exchange: Exchange) -> Arc<RingBuffer> {
        match exchange {
            Exchange::Binance | Exchange::BinanceFutures => self.binance_ring.clone(),
            Exchange::Bybit => self.bybit_ring.clone(),
            Exchange::OKX => self.okx_ring.clone(),
            _ => self.binance_ring.clone(), // Default to Binance
        }
    }

    /// Start processing thread (single thread for all symbols)
    pub fn start_processing(&mut self) {
        let binance_ring = self.binance_ring.clone();
        let bybit_ring = self.bybit_ring.clone();
        let okx_ring = self.okx_ring.clone();

        // Create processing thread
        thread::spawn(move || {
            // Pin to CPU core 2 on Windows
            #[cfg(windows)]
            set_thread_affinity(2);

            info!("HFT OrderBook processor started");

            // Pre-allocate orderbooks for fast access
            let mut orderbooks = Vec::with_capacity(MAX_SYMBOLS);
            for i in 0..MAX_SYMBOLS {
                orderbooks.push(FastOrderBook::new(i as u16));
            }

            let mut last_stats = Instant::now();
            let mut updates_since_stats = 0u64;

            loop {
                let mut processed = 0;

                // Process Binance updates
                while let Some(update) = binance_ring.pop() {
                    if (update.symbol_id as usize) < MAX_SYMBOLS {
                        orderbooks[update.symbol_id as usize].apply_update(&update);
                        processed += 1;
                    }
                }

                // Process Bybit updates
                while let Some(update) = bybit_ring.pop() {
                    if (update.symbol_id as usize) < MAX_SYMBOLS {
                        orderbooks[update.symbol_id as usize].apply_update(&update);
                        processed += 1;
                    }
                }

                // Process OKX updates
                while let Some(update) = okx_ring.pop() {
                    if (update.symbol_id as usize) < MAX_SYMBOLS {
                        orderbooks[update.symbol_id as usize].apply_update(&update);
                        processed += 1;
                    }
                }

                if processed > 0 {
                    updates_since_stats += processed;
                }

                // Log stats every second
                if last_stats.elapsed().as_secs() >= 1 {
                    let ups = updates_since_stats;
                    debug!("HFT OrderBook: {} updates/sec", ups);
                    updates_since_stats = 0;
                    last_stats = Instant::now();
                }

                // Small yield to prevent CPU spinning when no updates
                if processed == 0 {
                    std::hint::spin_loop();
                }
            }
        });
    }

    /// Get current orderbook for symbol
    pub fn get_orderbook(&self, symbol: &str) -> Option<FastOrderBookSnapshot> {
        let symbol_id = *self.symbol_to_id.get(symbol)?;
        let book = &self.orderbooks[symbol_id as usize];

        Some(FastOrderBookSnapshot {
            symbol: symbol.to_string(),
            bids: book.bids[0..book.bid_count as usize].to_vec(),
            asks: book.asks[0..book.ask_count as usize].to_vec(),
            last_update_id: book.last_update_id,
            tick_size: self.tick_sizes[symbol_id as usize],
        })
    }
}

/// Snapshot of orderbook for external use
pub struct FastOrderBookSnapshot {
    pub symbol: String,
    pub bids: Vec<FastPriceLevel>,
    pub asks: Vec<FastPriceLevel>,
    pub last_update_id: u64,
    pub tick_size: f64,
}

impl FastOrderBookSnapshot {
    /// Convert to market_types::OrderBook
    pub fn to_orderbook(&self, exchange: Exchange) -> market_types::OrderBook {
        let mut book = market_types::OrderBook::new(self.symbol.clone(), exchange);

        // Convert bids
        for level in &self.bids {
            book.bids.push(PriceLevel {
                price: level.price_ticks as f64 * self.tick_size,
                quantity: level.quantity as f64 / 1e8,
            });
        }

        // Convert asks
        for level in &self.asks {
            book.asks.push(PriceLevel {
                price: level.price_ticks as f64 * self.tick_size,
                quantity: level.quantity as f64 / 1e8,
            });
        }

        book.last_update_id = self.last_update_id;
        book
    }
}

/// Set thread affinity on Windows
#[cfg(windows)]
fn set_thread_affinity(core: usize) {
    // Simplified - actual implementation would use Windows API
    info!("Thread affinity would be set to core {} (requires winapi)", core);
}

#[cfg(not(windows))]
fn set_thread_affinity(core: usize) {
    // No-op on non-Windows
    let _ = core;
}