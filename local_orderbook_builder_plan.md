# Local Orderbook Builder Development Plan

## Executive Summary
Comprehensive plan for developing a production-grade local orderbook builder for the TradingPlatform, comparing current implementation with best practices from `local_orderbook_builder.md`.

## Current State Analysis

### ✅ What Exists
- Basic orderbook data structures with bids/asks
- Matching engine with L2→L3 conversion
- WebSocket feeds from multiple exchanges
- UDP multicast distribution
- Basic utility methods (best_bid, best_ask, mid_price, spread, WAP)

### ❌ Critical Gaps

| Gap | Impact | Priority |
|-----|--------|----------|
| No sequence number tracking | Cannot detect gaps/packet loss | CRITICAL |
| No REST snapshot initialization | Race conditions on startup | CRITICAL |
| No buffer management | Lost messages during init | CRITICAL |
| No gap detection & resync | Silent orderbook drift | CRITICAL |
| Using Vec instead of BTreeMap | O(n) vs O(log n) performance | HIGH |
| No per-symbol isolation | Lock contention | HIGH |
| No checksum validation | Cannot detect corruption | MEDIUM |
| No periodic validation | Drift goes undetected | MEDIUM |

## Development Plan

## Phase 1: Foundation (Critical - Week 1)

### Step 1: Add Sequence Number Infrastructure
**Files to modify:**
- `crates/market-types/src/orderbook.rs`

**Changes:**
```rust
pub struct OrderBook {
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub timestamp: Timestamp,
    pub last_update_id: u64,  // NEW: Track sequence
    pub bids: Vec<PriceLevel>, // Will change to BTreeMap later
    pub asks: Vec<PriceLevel>, // Will change to BTreeMap later
}

pub struct OrderBookUpdate {
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub timestamp: Timestamp,
    pub update_id: u64,        // NEW: Update sequence
    pub first_update_id: u64,  // NEW: For snapshot ranges
    pub is_snapshot: bool,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
}
```

**Exchange-specific sequence fields:**
- Binance: `lastUpdateId`, `U` (first), `u` (last)
- Bybit: `u` (update_id), `seq`
- OKX: `seqId`, `prevSeqId`

### Step 2: Implement REST Snapshot Support

**Files to create/modify:**
- `crates/feeder/src/crypto/rest/binance.rs`
- `crates/feeder/src/crypto/rest/bybit.rs`
- `crates/feeder/src/crypto/rest/okx.rs`

**Implementation pattern:**
```rust
#[async_trait]
impl ExchangeRest for BinanceRest {
    async fn get_orderbook(&self, symbol: &str, depth: usize) -> Result<OrderBookSnapshot> {
        let url = format!("{}/api/v3/depth?symbol={}&limit={}",
                         self.base_url, symbol, depth);
        let response = self.client.get(&url).send().await?;
        let data: BinanceDepth = response.json().await?;

        Ok(OrderBookSnapshot {
            symbol: symbol.to_string(),
            last_update_id: data.last_update_id,
            bids: data.bids,
            asks: data.asks,
            timestamp: Utc::now(),
        })
    }
}
```

### Step 3: Build Initialization State Machine

**New file:** `crates/matching-engine/src/orderbook/state.rs`

```rust
pub enum OrderBookState {
    /// Buffering diffs while waiting for snapshot
    Buffering {
        diffs: Vec<OrderBookUpdate>,
        started_at: Instant,
    },

    /// Processing snapshot and applying buffered diffs
    Initializing {
        snapshot_seq: u64,
        buffered_diffs: Vec<OrderBookUpdate>,
    },

    /// Normal operation with sequence tracking
    Live {
        last_seq: u64,
        last_update: Instant,
    },
}

pub struct OrderBookBuilder {
    symbol: String,
    exchange: Exchange,
    state: OrderBookState,
    book: OrderBook,
    rest_client: Box<dyn ExchangeRest>,
    resync_backoff: ExponentialBackoff,
}
```

## Phase 2: Reliability (High Priority - Week 2)

### Step 4: Gap Detection & Resync Logic

**Implementation in `process_update()`:**
```rust
impl OrderBookBuilder {
    pub async fn process_update(&mut self, update: OrderBookUpdate) -> Result<()> {
        match &self.state {
            OrderBookState::Live { last_seq, .. } => {
                // Check for sequence gap
                if !update.is_snapshot && update.update_id != last_seq + 1 {
                    warn!("Sequence gap detected: expected {}, got {}",
                          last_seq + 1, update.update_id);
                    self.trigger_resync().await?;
                    return Ok(());
                }

                // Apply update
                self.book.apply_update(&update)?;
                self.state = OrderBookState::Live {
                    last_seq: update.update_id,
                    last_update: Instant::now(),
                };
            }
            OrderBookState::Buffering { diffs, .. } => {
                // Buffer the diff
                diffs.push(update);

                // Check if we should fetch snapshot
                if diffs.len() == 1 {
                    self.fetch_snapshot().await?;
                }
            }
            OrderBookState::Initializing { snapshot_seq, buffered_diffs } => {
                // Apply buffered diffs with seq > snapshot_seq
                for diff in buffered_diffs.drain(..) {
                    if diff.update_id > *snapshot_seq {
                        self.book.apply_update(&diff)?;
                    }
                }

                self.state = OrderBookState::Live {
                    last_seq: self.book.last_update_id,
                    last_update: Instant::now(),
                };
            }
        }
        Ok(())
    }

    async fn trigger_resync(&mut self) -> Result<()> {
        // Apply backoff
        let delay = self.resync_backoff.next_backoff()
            .ok_or_else(|| anyhow!("Max resync attempts reached"))?;
        tokio::time::sleep(delay).await;

        // Reset to buffering state
        self.state = OrderBookState::Buffering {
            diffs: Vec::new(),
            started_at: Instant::now(),
        };

        // Fetch new snapshot
        self.fetch_snapshot().await
    }
}
```

### Step 5: Optimize Data Structures

**Update `OrderBook` struct:**
```rust
use std::collections::BTreeMap;
use std::cmp::Reverse;

pub struct OrderBook {
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub timestamp: Timestamp,
    pub last_update_id: u64,
    pub bids: BTreeMap<Reverse<u64>, f64>,  // Sorted descending
    pub asks: BTreeMap<u64, f64>,           // Sorted ascending
}

impl OrderBook {
    pub fn apply_level(&mut self, side: Side, price: u64, quantity: f64) {
        match side {
            Side::Bid => {
                if quantity == 0.0 {
                    self.bids.remove(&Reverse(price));
                } else {
                    self.bids.insert(Reverse(price), quantity);
                }
            }
            Side::Ask => {
                if quantity == 0.0 {
                    self.asks.remove(&price);
                } else {
                    self.asks.insert(price, quantity);
                }
            }
        }
    }

    pub fn best_bid(&self) -> Option<(u64, f64)> {
        self.bids.iter().next()
            .map(|(Reverse(price), qty)| (*price, *qty))
    }

    pub fn best_ask(&self) -> Option<(u64, f64)> {
        self.asks.iter().next()
            .map(|(price, qty)| (*price, *qty))
    }
}
```

### Step 6: Per-Symbol Architecture

**New architecture pattern:**
```rust
pub struct SymbolManager {
    builders: HashMap<String, tokio::task::JoinHandle<()>>,
    senders: HashMap<String, mpsc::Sender<OrderBookUpdate>>,
}

impl SymbolManager {
    pub async fn add_symbol(&mut self, symbol: String, exchange: Exchange) {
        let (tx, mut rx) = mpsc::channel(1000);

        // Spawn dedicated task for this symbol
        let handle = tokio::spawn(async move {
            let mut builder = OrderBookBuilder::new(symbol, exchange);

            while let Some(update) = rx.recv().await {
                if let Err(e) = builder.process_update(update).await {
                    error!("Failed to process update: {}", e);
                    // Trigger resync
                    builder.trigger_resync().await;
                }

                // Publish to downstream
                builder.publish_snapshot().await;
            }
        });

        self.builders.insert(symbol.clone(), handle);
        self.senders.insert(symbol, tx);
    }
}
```

## Phase 3: Production Hardening (Medium Priority - Week 3)

### Step 7: Add Validation Systems

**Checksum validation (OKX example):**
```rust
fn validate_checksum(&self, book: &OrderBook, expected_crc32: u32) -> bool {
    let mut checksum_str = String::new();

    // Take top 25 levels each side
    for (Reverse(price), qty) in book.bids.iter().take(25) {
        checksum_str.push_str(&format!("{}:{}", price, qty));
    }

    for (price, qty) in book.asks.iter().take(25) {
        checksum_str.push_str(&format!("{}:{}", price, qty));
    }

    let calculated_crc = crc32fast::hash(checksum_str.as_bytes());
    calculated_crc == expected_crc32
}
```

**Crossed book detection:**
```rust
fn validate_book_integrity(&self, book: &OrderBook) -> Result<()> {
    if let (Some((bid_price, _)), Some((ask_price, _))) =
        (book.best_bid(), book.best_ask()) {
        if bid_price >= ask_price {
            return Err(anyhow!("Crossed book detected: bid {} >= ask {}",
                              bid_price, ask_price));
        }
    }
    Ok(())
}
```

**Background validator:**
```rust
pub struct OrderBookValidator {
    rest_client: Box<dyn ExchangeRest>,
    check_interval: Duration,
}

impl OrderBookValidator {
    pub async fn run(&self, book: Arc<RwLock<OrderBook>>) {
        let mut interval = tokio::time::interval(self.check_interval);

        loop {
            interval.tick().await;

            // Get REST snapshot
            let snapshot = self.rest_client
                .get_orderbook(&book.read().symbol, 20)
                .await?;

            // Compare top levels
            let book_guard = book.read();
            if !self.compare_top_levels(&*book_guard, &snapshot) {
                warn!("Orderbook drift detected for {}", book_guard.symbol);
                // Trigger resync
            }
        }
    }
}
```

### Step 8: Recovery & Persistence

**Snapshot persistence:**
```rust
use bincode;
use std::fs;

impl OrderBook {
    pub fn save_snapshot(&self, path: &Path) -> Result<()> {
        let encoded = bincode::serialize(&self)?;
        fs::write(path, encoded)?;
        Ok(())
    }

    pub fn load_snapshot(path: &Path) -> Result<Self> {
        let data = fs::read(path)?;
        let book = bincode::deserialize(&data)?;
        Ok(book)
    }
}
```

### Step 9: Windows-Specific Optimizations

**Configure for Windows:**
```rust
#[cfg(target_os = "windows")]
pub fn configure_socket(socket: &UdpSocket) -> Result<()> {
    use winapi::um::winsock2::*;

    // Increase receive buffer
    socket.set_recv_buffer_size(4 * 1024 * 1024)?;

    // Enable multicast loopback
    socket.set_multicast_loop_v4(true)?;

    // Set socket to non-blocking for IOCP
    socket.set_nonblocking(true)?;

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn set_thread_affinity(core_id: usize) {
    use winapi::um::processthreadsapi::*;
    use winapi::um::winbase::*;

    unsafe {
        let mask = 1usize << core_id;
        SetThreadAffinityMask(GetCurrentThread(), mask);
    }
}
```

## Phase 4: Testing & Monitoring (Week 4)

### Step 10: Comprehensive Testing

**Test scenarios:**
```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_sequence_gap_detection() {
        let mut builder = OrderBookBuilder::new("BTCUSDT", Exchange::Binance);

        // Apply update with sequence 100
        let update1 = create_update(100);
        builder.process_update(update1).await.unwrap();

        // Apply update with gap (102 instead of 101)
        let update2 = create_update(102);
        let result = builder.process_update(update2).await;

        // Should trigger resync
        assert!(matches!(builder.state, OrderBookState::Buffering { .. }));
    }

    #[tokio::test]
    async fn test_snapshot_initialization() {
        // Test the full initialization flow
        // 1. Start buffering
        // 2. Receive snapshot
        // 3. Apply buffered diffs correctly
    }
}
```

### Step 11: Monitoring & Metrics

**Metrics to track:**
```rust
use prometheus::{Counter, Histogram, Gauge};

pub struct OrderBookMetrics {
    pub updates_processed: Counter,
    pub sequence_gaps: Counter,
    pub resyncs_triggered: Counter,
    pub update_latency: Histogram,
    pub book_depth: Gauge,
    pub last_update_timestamp: Gauge,
}

impl OrderBookBuilder {
    fn record_metrics(&self, update: &OrderBookUpdate) {
        self.metrics.updates_processed.inc();

        let latency = Instant::now() - update.timestamp;
        self.metrics.update_latency.observe(latency.as_secs_f64());

        self.metrics.book_depth.set(self.book.bids.len() as f64);
    }
}
```

## Implementation Timeline

| Week | Phase | Deliverables |
|------|-------|--------------|
| 1 | Foundation | Sequence tracking, REST snapshots, buffer management |
| 2 | Reliability | Gap detection, BTreeMap migration, per-symbol tasks |
| 3 | Production | Validation, persistence, Windows optimizations |
| 4 | Testing | Unit tests, integration tests, monitoring setup |

## Success Criteria

- ✅ Zero silent orderbook drift
- ✅ Automatic recovery from all gap scenarios
- ✅ Sub-millisecond update processing
- ✅ Support for 1000+ symbols across 10 exchanges
- ✅ 99.99% orderbook accuracy vs exchange REST API
- ✅ Graceful degradation during exchange issues
- ✅ Comprehensive monitoring and alerting

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| REST rate limits | Exponential backoff, rate limit tracking per exchange |
| Memory growth | Bounded depth, periodic cleanup, memory monitoring |
| Network partitions | Timeout detection, automatic reconnection |
| Exchange API changes | Version detection, graceful fallback |

## Performance Targets

- **Latency**: < 100µs for update processing
- **Throughput**: > 100k updates/sec per core
- **Memory**: < 1MB per symbol (top 100 levels)
- **Recovery**: < 1s resync time
- **Accuracy**: 100% sequence tracking

## Notes for Windows Environment

Since you're on Windows, key considerations:
- Use Tokio with IOCP backend (automatic)
- Configure Winsock2 buffer sizes explicitly
- Use `SetThreadAffinityMask` for CPU pinning if needed
- Test multicast with Windows Firewall configured
- Consider using binary protocol for internal communication

## Next Steps

1. **Start with Binance** as proof of concept (most mature API)
2. **Implement sequence tracking** first (foundation for everything else)
3. **Add REST snapshot support** for one symbol
4. **Test gap detection** thoroughly before scaling
5. **Apply pattern** to other exchanges incrementally