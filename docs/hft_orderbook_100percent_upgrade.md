# Ultra-Low Latency HFT OrderBook System (100% HFT-Grade)

**Goal**: Upgrade the current orderbook system to match top-tier HFT firms (Jump Trading, Citadel, Tower Research, Jane Street)

## Current vs Target Performance

| Metric | Current | Target (100%) | Improvement |
|--------|---------|---------------|-------------|
| p99 Latency | 5-10µs | 0.5-2µs | 5-10x faster |
| Throughput | 100K updates/sec | 500K+ updates/sec | 5x higher |
| Lock Contention | Moderate (RwLock) | Zero (lock-free) | Eliminated |
| CPU Efficiency | Good | Excellent | Better utilization |
| Scalability | 7 exchanges | 20+ exchanges | 3x more |

---

## Phase 1: Lock-Free Architecture

### Problem with Current Design
- Uses `Arc<RwLock<Box<[FastOrderBook; 2048]>>>`
- Write locks block all readers
- Lock contention between processing threads
- ~50-100ns overhead per lock acquisition

### Solution: Seqlock Pattern

```rust
/// Lock-free orderbook using seqlock pattern
struct SeqLockOrderBook {
    sequence: AtomicU64,  // Even = stable, Odd = being written
    data: UnsafeCell<FastOrderBook>,
}

impl SeqLockOrderBook {
    /// Lock-free read (readers never block)
    pub fn read(&self) -> FastOrderBook {
        loop {
            let seq1 = self.sequence.load(Ordering::Acquire);
            if seq1 % 2 == 1 {
                // Writer is active, spin
                std::hint::spin_loop();
                continue;
            }

            // Safe to read
            let data = unsafe { (*self.data.get()).clone() };

            // Verify no writer interfered
            let seq2 = self.sequence.load(Ordering::Acquire);
            if seq1 == seq2 {
                return data;  // Clean read
            }
            // Retry if writer updated during read
        }
    }

    /// Lock-free write (single writer assumed)
    pub fn write(&self, update: FastOrderBook) {
        // Increment to odd (writers start)
        let seq = self.sequence.fetch_add(1, Ordering::Release);

        // Update data
        unsafe {
            *self.data.get() = update;
        }

        // Increment to even (writers done)
        self.sequence.fetch_add(1, Ordering::Release);
    }
}
```

**Benefits**:
- Readers never block: ~10-20ns overhead (vs 50-100ns with RwLock)
- Writers don't block readers
- Cache-friendly: sequence check is very fast

**Tradeoff**:
- Readers may retry if writer is active
- Assumes single writer per orderbook (OK for us - one thread per symbol)

---

## Phase 2: Advanced Threading Model

### Tiered Work-Stealing Architecture

```rust
struct TieredHFTProcessor {
    // Tier 1: Hot symbols (dedicated threads, no sharing)
    hot_tier: Vec<DedicatedProcessor>,  // 5 threads

    // Tier 2: Warm symbols (work-stealing pool)
    warm_tier: WorkStealingPool,  // 8 threads with deques

    // Tier 3: Cold symbols (single batch processor)
    cold_tier: BatchProcessor,  // 1 thread

    // Dynamic classifier
    classifier: SymbolClassifier,
}

/// Work-stealing deque (like Tokio/Rayon)
struct WorkStealingPool {
    workers: Vec<Worker>,
    stealers: Vec<Stealer>,
}

impl Worker {
    fn run(&self) {
        loop {
            // Try own queue first
            if let Some(update) = self.queue.pop() {
                process(update);
            } else {
                // Steal from random other worker
                let victim = rand::random::<usize>() % self.stealers.len();
                if let Some(update) = self.stealers[victim].steal() {
                    process(update);
                } else {
                    // No work anywhere, yield
                    std::thread::yield_now();
                }
            }
        }
    }
}
```

### Symbol Classification

**Static Classification** (at startup):
```rust
const HOT_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "BNBUSDT", "SOLUSDT", "XRPUSDT",
    "DOGEUSDT", "ADAUSDT", "AVAXUSDT", "DOTUSDT", "MATICUSDT",
];

const WARM_THRESHOLD: usize = 10;  // updates/sec
const COLD_THRESHOLD: usize = 1;   // updates/sec
```

**Dynamic Reclassification** (every 60 seconds):
```rust
impl SymbolClassifier {
    fn reclassify(&mut self) {
        for symbol in self.all_symbols() {
            let rate = self.measure_update_rate(symbol);

            match rate {
                r if r > 100 => self.promote_to_hot(symbol),
                r if r > 10 => self.assign_to_warm(symbol),
                _ => self.demote_to_cold(symbol),
            }
        }
    }
}
```

### CPU Core Pinning

**Windows Implementation**:
```rust
#[cfg(windows)]
fn pin_thread_to_core(core_id: usize) {
    use winapi::um::processthreadsapi::SetThreadAffinityMask;
    use winapi::um::winbase::GetCurrentThread;

    let mask = 1u64 << core_id;
    unsafe {
        SetThreadAffinityMask(GetCurrentThread(), mask);
    }
}

// Pin hot threads to cores 2-6 (avoid core 0-1 for OS)
for (i, thread) in hot_threads.iter().enumerate() {
    pin_thread_to_core(2 + i);
}
```

**NUMA-Aware Allocation**:
```rust
#[cfg(windows)]
fn allocate_numa_memory(size: usize, node: u32) -> *mut u8 {
    use winapi::um::memoryapi::VirtualAllocExNuma;

    unsafe {
        VirtualAllocExNuma(
            GetCurrentProcess(),
            std::ptr::null_mut(),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
            node,
        ) as *mut u8
    }
}
```

---

## Phase 3: Memory Optimization

### Cache-Line Padding

```rust
#[repr(C, align(64))]
struct CacheAlignedOrderBook {
    // Hot data (first cache line)
    sequence: AtomicU64,
    symbol_id: u16,
    _pad1: [u8; 54],  // Pad to 64 bytes

    // Orderbook data (next cache lines)
    bids: [FastPriceLevel; 20],
    asks: [FastPriceLevel; 20],

    // Stats (separate cache line to avoid false sharing)
    _pad2: [u8; 64],
    update_count: AtomicU64,
    last_update_time: AtomicU64,
}
```

**Why**: Prevents false sharing when multiple threads access adjacent orderbooks.

### Memory Pools

```rust
struct OrderBookPool {
    // Pre-allocated pool
    books: Vec<Box<FastOrderBook>>,
    free_list: AtomicPtr<FastOrderBook>,
}

impl OrderBookPool {
    fn allocate(&self) -> *mut FastOrderBook {
        // Lock-free allocation from pool
        loop {
            let ptr = self.free_list.load(Ordering::Acquire);
            if ptr.is_null() {
                panic!("Pool exhausted");
            }

            let next = unsafe { (*ptr).next };
            if self.free_list.compare_exchange(ptr, next,
                Ordering::Release, Ordering::Acquire).is_ok() {
                return ptr;
            }
        }
    }
}
```

### Double Buffering

```rust
struct DoubleBufferedOrderBook {
    buffers: [UnsafeCell<FastOrderBook>; 2],
    current: AtomicU8,  // 0 or 1
}

impl DoubleBufferedOrderBook {
    fn read(&self) -> &FastOrderBook {
        let idx = self.current.load(Ordering::Acquire);
        unsafe { &*self.buffers[idx as usize].get() }
    }

    fn write(&self, update: FastOrderBook) {
        let current = self.current.load(Ordering::Acquire);
        let next = 1 - current;

        // Write to inactive buffer
        unsafe { *self.buffers[next as usize].get() = update; }

        // Atomic swap
        self.current.store(next, Ordering::Release);
    }
}
```

---

## Phase 4: Latency Reduction Techniques

### 1. Batched Updates

Instead of processing one update at a time, batch them:

```rust
impl WarmTierWorker {
    fn process_batch(&self) {
        const BATCH_SIZE: usize = 16;
        let mut batch = Vec::with_capacity(BATCH_SIZE);

        // Collect batch
        while batch.len() < BATCH_SIZE {
            if let Some(update) = self.queue.try_pop() {
                batch.push(update);
            } else {
                break;
            }
        }

        // Process batch (amortize overhead)
        for update in batch {
            self.apply_update(update);
        }
    }
}
```

**Benefit**: Amortizes cache misses, branch predictions, etc.

### 2. CPU Cache Prefetching

```rust
impl FastOrderBook {
    #[inline(always)]
    fn apply_update_with_prefetch(&mut self, update: &FastUpdate) {
        // Prefetch next orderbook
        unsafe {
            std::intrinsics::prefetch_read_data(
                next_orderbook_ptr,
                3  // High temporal locality
            );
        }

        // Process current
        self.apply_update(update);
    }
}
```

### 3. SIMD Optimization (Advanced)

For finding price levels in sorted arrays:

```rust
#[cfg(target_arch = "x86_64")]
unsafe fn find_price_simd(levels: &[FastPriceLevel], price: i64) -> Option<usize> {
    use std::arch::x86_64::*;

    // Load 4 prices at once and compare
    for i in (0..levels.len()).step_by(4) {
        let prices = _mm256_loadu_si256(
            &levels[i].price_ticks as *const i64 as *const __m256i
        );
        let target = _mm256_set1_epi64x(price);
        let cmp = _mm256_cmpeq_epi64(prices, target);
        let mask = _mm256_movemask_epi8(cmp);

        if mask != 0 {
            return Some(i + (mask.trailing_zeros() / 8) as usize);
        }
    }
    None
}
```

---

## Phase 5: Monitoring & Dynamic Optimization

### Latency Histogram

```rust
use hdrhistogram::Histogram;

struct LatencyTracker {
    hot_latency: Histogram<u64>,
    warm_latency: Histogram<u64>,
    cold_latency: Histogram<u64>,
}

impl LatencyTracker {
    fn record_update(&mut self, tier: Tier, latency_ns: u64) {
        match tier {
            Tier::Hot => self.hot_latency.record(latency_ns).ok(),
            Tier::Warm => self.warm_latency.record(latency_ns).ok(),
            Tier::Cold => self.cold_latency.record(latency_ns).ok(),
        };
    }

    fn report(&self) {
        info!("Hot tier: p50={:.1}µs p99={:.1}µs p999={:.1}µs",
            self.hot_latency.value_at_quantile(0.5) as f64 / 1000.0,
            self.hot_latency.value_at_quantile(0.99) as f64 / 1000.0,
            self.hot_latency.value_at_quantile(0.999) as f64 / 1000.0,
        );
    }
}
```

### Queue Depth Monitoring

```rust
impl WorkStealingPool {
    fn check_saturation(&self) -> bool {
        let total_depth: usize = self.workers.iter()
            .map(|w| w.queue.len())
            .sum();

        let avg_depth = total_depth / self.workers.len();

        if avg_depth > 1000 {
            warn!("Warm tier saturated! Avg queue depth: {}", avg_depth);
            true
        } else {
            false
        }
    }
}
```

### Dynamic Rebalancing

```rust
struct AdaptiveClassifier {
    symbol_stats: HashMap<String, SymbolStats>,
    rebalance_interval: Duration,
}

impl AdaptiveClassifier {
    async fn rebalance_loop(&mut self) {
        let mut interval = tokio::time::interval(self.rebalance_interval);

        loop {
            interval.tick().await;

            // Measure actual load
            for (symbol, stats) in &self.symbol_stats {
                let updates_per_sec = stats.updates_last_minute() / 60.0;
                let avg_latency = stats.avg_latency_us();

                // Promote if high rate and acceptable latency
                if updates_per_sec > 50.0 && avg_latency < 10.0 {
                    self.promote_to_hot(symbol);
                }

                // Demote if low rate
                if updates_per_sec < 5.0 {
                    self.demote_to_warm_or_cold(symbol);
                }
            }

            info!("Rebalanced: {} hot, {} warm, {} cold",
                self.hot_symbols.len(),
                self.warm_symbols.len(),
                self.cold_symbols.len()
            );
        }
    }
}
```

---

## Phase 6: Windows-Specific Optimizations

### Process Priority

```rust
#[cfg(windows)]
fn set_realtime_priority() {
    use winapi::um::processthreadsapi::{
        SetPriorityClass, SetThreadPriority, GetCurrentProcess, GetCurrentThread
    };
    use winapi::um::winbase::{
        REALTIME_PRIORITY_CLASS, THREAD_PRIORITY_TIME_CRITICAL
    };

    unsafe {
        // Process priority
        SetPriorityClass(GetCurrentProcess(), REALTIME_PRIORITY_CLASS);

        // Thread priority (for hot tier threads)
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    }

    warn!("Running with REALTIME priority - may impact system responsiveness");
}
```

### High-Resolution Timing

```rust
#[cfg(windows)]
fn get_timestamp_ns() -> u64 {
    use winapi::um::profileapi::{QueryPerformanceCounter, QueryPerformanceFrequency};

    unsafe {
        let mut counter: i64 = 0;
        let mut frequency: i64 = 0;

        QueryPerformanceCounter(&mut counter);
        QueryPerformanceFrequency(&mut frequency);

        // Convert to nanoseconds
        (counter as u128 * 1_000_000_000 / frequency as u128) as u64
    }
}

// Use RDTSC for even lower overhead (CPU cycles)
#[inline(always)]
fn rdtsc() -> u64 {
    unsafe { std::arch::x86_64::_rdtsc() }
}
```

### Huge Pages (Large Pages)

```rust
#[cfg(windows)]
fn allocate_large_pages(size: usize) -> *mut u8 {
    use winapi::um::memoryapi::VirtualAlloc;
    use winapi::um::winnt::{MEM_COMMIT, MEM_RESERVE, MEM_LARGE_PAGES, PAGE_READWRITE};

    unsafe {
        VirtualAlloc(
            std::ptr::null_mut(),
            size,
            MEM_COMMIT | MEM_RESERVE | MEM_LARGE_PAGES,
            PAGE_READWRITE,
        ) as *mut u8
    }
}
```

**Note**: Requires `SeLockMemoryPrivilege` in Windows

---

## Implementation Roadmap

### Phase 1: Foundation (Week 1)
- [ ] Implement Seqlock orderbooks
- [ ] Replace RwLock with Seqlock
- [ ] Add cache-line padding
- [ ] Benchmark seqlock vs RwLock

### Phase 2: Threading (Week 2)
- [ ] Implement tiered processor (hot/warm/cold)
- [ ] Add work-stealing for warm tier
- [ ] Implement CPU core pinning
- [ ] Static symbol classification

### Phase 3: Optimization (Week 3)
- [ ] Add double buffering for hot symbols
- [ ] Implement batched updates
- [ ] Add memory pools
- [ ] NUMA-aware allocation

### Phase 4: Monitoring (Week 4)
- [ ] Add latency histograms
- [ ] Queue depth monitoring
- [ ] Dynamic symbol reclassification
- [ ] Performance dashboards

### Phase 5: Windows Tuning (Week 5)
- [ ] Process/thread priority
- [ ] High-resolution timing
- [ ] Large pages support
- [ ] NUMA node pinning

### Phase 6: Advanced (Week 6+)
- [ ] SIMD optimizations
- [ ] CPU cache prefetching
- [ ] Lock-free memory pools
- [ ] Zero-copy updates

---

## Expected Results

### Performance Improvements

| Component | Before | After | Gain |
|-----------|--------|-------|------|
| OrderBook update | 5-10µs | 0.5-2µs | 5-10x |
| OrderBook read | 1-2µs | 0.05-0.2µs | 10-20x |
| Throughput | 100K/sec | 500K+/sec | 5x |
| CPU usage | 70-80% | 50-60% | More efficient |
| Lock contention | Moderate | Zero | Eliminated |

### Scalability

- **Current**: 7 exchanges, ~1500 symbols
- **Target**: 20+ exchanges, 5000+ symbols
- **Headroom**: Can handle 10x growth without architecture changes

---

## References & Resources

### Papers
- "Seqlocks" - Linux Kernel Documentation
- "Work-Stealing Queue" - Tokio/Rayon design docs
- "LMAX Disruptor" - Martin Thompson
- "Lock-Free Programming" - Herb Sutter

### Libraries to Consider
- `crossbeam` - Lock-free data structures
- `parking_lot` - Fast synchronization primitives
- `hdrhistogram` - Latency tracking
- `arrayvec` - Stack-allocated vectors
- `smallvec` - Small vector optimization

### HFT Blogs & Resources
- Mechanical Sympathy (Martin Thompson)
- "What Every Programmer Should Know About Memory" (Ulrich Drepper)
- Linux perf tools documentation
- Intel optimization manuals

---

## Trade-offs & Considerations

### Complexity vs Performance
- **Simple RwLock**: Easy to understand, but slower
- **Seqlock**: Medium complexity, much faster
- **Lock-free**: High complexity, highest performance

### Memory vs Speed
- More memory pools = faster allocation
- Cache-line padding = more memory, less false sharing
- Double buffering = 2x memory, zero read blocking

### Determinism vs Throughput
- Work-stealing = high throughput, variable latency
- Dedicated threads = lower throughput, predictable latency
- Hybrid (our approach) = balance both

### Development Time
- Phases 1-2: Core improvements, ~70% of gains
- Phases 3-4: Optimizations, ~20% of gains
- Phases 5-6: Fine-tuning, ~10% of gains

**Recommendation**: Implement phases incrementally, measure at each step.

---

## Maintenance & Operations

### Monitoring Checklist
- [ ] Queue depths per tier
- [ ] Latency percentiles (p50, p99, p999)
- [ ] CPU utilization per core
- [ ] Symbol classification (hot/warm/cold counts)
- [ ] Cache miss rates
- [ ] Context switch rates

### Tuning Parameters
```toml
[hft_orderbook]
hot_tier_threads = 5
warm_tier_threads = 8
cold_tier_threads = 1

hot_threshold_updates_per_sec = 100
warm_threshold_updates_per_sec = 10

rebalance_interval_sec = 60
batch_size = 16

enable_numa = true
enable_huge_pages = false  # Requires admin privileges
enable_realtime_priority = false  # Use with caution
```

### Troubleshooting

**High Latency**:
- Check queue depths (saturation?)
- Check CPU affinity (cores isolated?)
- Check for lock contention (use perf/vtune)

**Low Throughput**:
- Too few threads?
- Symbols misclassified?
- CPU frequency scaling disabled?

**Memory Issues**:
- Pool exhaustion?
- Memory leaks in update path?
- Check with valgrind/Windows Performance Analyzer

---

**Last Updated**: 2025-10-16
**Status**: Design Document - Ready for Implementation
