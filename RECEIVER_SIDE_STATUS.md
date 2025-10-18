# Receiver Side - Current Status & Next Steps

## What Already Exists ✅

### 1. **UDP Protocol** (`udp-protocol` crate) ✅
**Status**: Fully implemented with binary protocol

**Components**:
- `sender.rs` - UDP multicast sender (text/binary)
- `receiver.rs` - UDP receiver with packet parsing
- `database_receiver.rs` - Database-specific receiver
- `codec.rs` - Binary protocol encoding/decoding

**Protocol**:
```rust
struct UdpPacketHeader {
    magic: u32,           // Magic: 0x48465450 ("HFTP")
    sequence: u64,        // Packet sequence number
    timestamp: i64,       // Microseconds since epoch
    exchange_id: u8,      // Exchange identifier
    message_type: u8,     // 0: OrderBook, 1: Trade
    symbol_len: u16,      // Symbol string length
}
```

**Features**:
- Packet loss detection (sequence numbers)
- Parses OrderBookUpdate and Trade messages
- Async/await with tokio
- Shutdown handling

### 2. **Matching Engine** (`matching-engine` crate) ✅
**Status**: Production-grade HFT implementation

**Key File**: `hft_orderbook.rs` (23KB)

**Architecture**:
- **Lock-free SPSC ring buffers** (65K capacity)
- **Batch processing** (16x reduction in lock overhead)
- **Cache-optimized** memory layout (64-byte alignment)
- **CPU prefetching** for next orderbook
- **Integer-only arithmetic** (no f64 in hot path)
- **CPU core pinning** (Linux: core 2)
- **Zero allocations** in hot path

**Performance**:
```rust
const MAX_SYMBOLS: usize = 2048;      // Handle 2K symbols
const MAX_DEPTH: usize = 20;          // Top 20 levels per side
const RING_BUFFER_SIZE: usize = 65536; // 65K update buffer
const BATCH_SIZE: usize = 16;         // Process 16 updates per lock
```

**Structure**:
```rust
pub struct HFTOrderBookProcessor {
    // Pre-allocated orderbooks for all symbols
    orderbooks: Arc<RwLock<Box<[FastOrderBook; MAX_SYMBOLS]>>>,

    // Lock-free ring buffers per exchange
    binance_ring: Arc<RingBuffer>,
    bybit_ring: Arc<RingBuffer>,
    okx_ring: Arc<RingBuffer>,

    // Symbol mapping
    symbol_to_id: HashMap<String, u16>,
}
```

**Features**:
- Convert L2 updates → Fast integer orderbook
- Single-threaded processing (all symbols)
- Batch processing (100ns ÷ 16 = 6ns per update)
- Cache prefetching (200 cycles → 4 cycles)
- Get orderbook snapshot

### 3. **Feature Engineering** (`feature-engineering` crate) ✅
**Status**: Core features implemented, needs OFI integration

**Key File**: `calculator.rs` (10KB)

**Implemented Features**:
```rust
pub struct MarketFeatures {
    // Weighted Average Price (lagged)
    wap_0ms: Option<f64>,
    wap_100ms: Option<f64>,
    wap_200ms: Option<f64>,   // 200ms lag as specified
    wap_300ms: Option<f64>,

    // Order Book Metrics
    order_book_imbalance: Option<f64>,    // (bid_qty - ask_qty) / total
    liquidity_imbalance: Option<f64>,      // Within depth threshold
    queue_imbalance: Option<f64>,          // Bid vs ask depth

    // Trade Flow Metrics
    tick_flow_imbalance: Option<f64>,      // TFI: buy_ticks - sell_ticks
    trade_flow_imbalance: Option<f64>,     // Volume-weighted TFI

    // Spread Metrics
    spread: Option<f64>,
    spread_bps: Option<f64>,

    // Volume at Depth
    bid_volume_10bps: Option<f64>,
    ask_volume_10bps: Option<f64>,
    bid_volume_20bps: Option<f64>,
    ask_volume_20bps: Option<f64>,

    // Order Flow Imbalance (framework exists)
    order_flow_imbalance: Option<f64>,     // TODO: Needs integration
}
```

**Components**:
- `calculator.rs` - Feature calculation logic
- `history.rs` - Circular buffers for time-series (OrderBookHistory, TradeHistory)
- `pipeline.rs` - Feature processing pipeline

### 4. **Other Crates** 📦
- `market-types` - Shared data structures (Exchange, OrderBook, Trade, etc.)
- `signal-generation` - Signal generation from features
- `backtesting` - Strategy backtesting
- `oms` - Order Management System
- `database` - Database adapters (QuestDB, KDB+)
- `historical-loader` - Historical data loading

---

## What Needs to Be Done 🔨

### Immediate Next Steps

#### 1. **Connect UDP Receiver → HFT OrderBook** 🔴 HIGH PRIORITY
**Status**: Need integration

**Implementation**:
```rust
// In matching-engine/src/main.rs or bin/consumer.rs
#[tokio::main]
async fn main() {
    // 1. Create HFT processor
    let mut processor = HFTOrderBookProcessor::new();

    // 2. Register symbols
    processor.register_symbol("BTC-USDT", 0.01);
    processor.register_symbol("ETH-USDT", 0.01);
    // ... register all symbols

    // 3. Start processing thread (single thread for all symbols)
    processor.start_processing();

    // 4. Create UDP receiver
    let exchanges = vec![
        (Exchange::Binance, 9001),
        (Exchange::Bybit, 9001),
        (Exchange::Coinbase, 9001),
    ];

    let (mut receiver, mut orderbook_rx, mut trade_rx) =
        UdpReceiver::new(exchanges, 10000);

    receiver.start().await?;

    // 5. Main event loop
    loop {
        tokio::select! {
            Some(update) = orderbook_rx.recv() => {
                // Convert to FastUpdate
                if let Some(fast_update) = processor.convert_update(&update) {
                    // Push to appropriate ring buffer
                    let ring = processor.get_ring_buffer(update.exchange);
                    if !ring.push(fast_update) {
                        warn!("Ring buffer full for {}", update.exchange);
                    }
                }
            }
            Some(trade) = trade_rx.recv() => {
                // Process trade (feature engineering)
                // TODO: Send to feature engine
            }
        }
    }
}
```

**Files to create**:
- `crates/matching-engine/src/bin/consumer.rs` - Main UDP consumer binary
- Or update existing binary if one exists

#### 2. **Integrate HFT OrderBook → Feature Engineering** 🔴 HIGH PRIORITY
**Status**: Need to connect the two

**Implementation**:
```rust
// Feature engine needs to read from HFT orderbook
pub struct FeatureEngine {
    hft_processor: Arc<HFTOrderBookProcessor>,
    calculator: FeatureCalculator,
}

impl FeatureEngine {
    pub fn calculate_features(&self, symbol: &str) -> Option<MarketFeatures> {
        // 1. Get current orderbook from HFT processor
        let snapshot = self.hft_processor.get_orderbook(symbol)?;

        // 2. Convert to market_types::OrderBook
        let orderbook = snapshot.to_orderbook(Exchange::Binance);

        // 3. Calculate features
        self.calculator.calculate_features(symbol.to_string(), Exchange::Binance)
    }
}
```

#### 3. **Cross-Exchange NBBO** 🟡 MEDIUM PRIORITY
**Status**: Need to implement

**Implementation**:
```rust
// In matching-engine or feature-engineering
pub fn calculate_nbbo(
    processor: &HFTOrderBookProcessor,
    symbol: &str,
    exchanges: &[Exchange]
) -> Option<(i64, i64)> {
    let mut best_bid = i64::MIN;
    let mut best_ask = i64::MAX;

    for exchange in exchanges {
        let ex_symbol = format!("{}-{}", symbol, exchange);
        if let Some(snapshot) = processor.get_orderbook(&ex_symbol) {
            if let Some(bid) = snapshot.bids.first() {
                best_bid = best_bid.max(bid.price_ticks);
            }
            if let Some(ask) = snapshot.asks.first() {
                best_ask = best_ask.min(ask.price_ticks);
            }
        }
    }

    if best_bid != i64::MIN && best_ask != i64::MAX {
        Some((best_bid, best_ask))
    } else {
        None
    }
}
```

#### 4. **Signal Generation Pipeline** 🟡 MEDIUM PRIORITY
**Status**: Needs implementation

```rust
pub struct SignalGenerator {
    feature_engine: FeatureEngine,
    models: HashMap<String, Box<dyn PredictionModel>>,
}

impl SignalGenerator {
    pub fn generate_signal(&mut self, symbol: &str) -> Option<Signal> {
        // 1. Get features
        let features = self.feature_engine.calculate_features(symbol)?;

        // 2. Predict 15-second forward price
        let model = self.models.get(symbol)?;
        let prediction = model.predict(&features);

        // 3. Generate trading signal
        Some(Signal {
            symbol: symbol.to_string(),
            action: self.determine_action(prediction),
            confidence: self.calculate_confidence(&features),
            timestamp: std::time::SystemTime::now(),
        })
    }
}
```

---

## Proposed Implementation Plan

### Phase 1: Connect the Pipeline (Week 1)
1. ✅ **Day 1-2**: Create `consumer` binary - **COMPLETED**
   - ✅ Created `crates/matching-engine/src/bin/consumer.rs`
   - ✅ UDP Receiver → HFT OrderBook integration implemented
   - ✅ Configuration system with `config/consumer/symbols.json`
   - ✅ Symbol registration from config
   - ✅ Compiled successfully
   - **Status**: Ready for testing with live data from feeder

2. ⏳ **Day 3-4**: HFT OrderBook → Feature Engineering
   - Connect processor to feature calculator
   - Test feature calculation with live orderbooks

3. ⏳ **Day 5-7**: Cross-exchange NBBO
   - Implement NBBO calculation
   - Test with multi-exchange data

### Phase 2: Feature Refinement (Week 2)
1. **OFI Integration**: Complete Order Flow Imbalance calculation
2. **WAP Validation**: Verify 200ms lag implementation
3. **Feature Testing**: Validate all features against known data

### Phase 3: Signal Generation (Week 3)
1. **Model Interface**: Define prediction model interface
2. **Simple Model**: Implement basic linear model for testing
3. **Signal Logic**: Implement signal generation rules

### Phase 4: Production Ready (Week 4)
1. **Performance Tuning**: Optimize latency
2. **Monitoring**: Add metrics and dashboards
3. **Testing**: Stress test with high-frequency data
4. **Documentation**: Document the entire pipeline

---

## Performance Targets

Based on the HFT implementation:
- **Orderbook Update Latency**: < 100µs (sub-microsecond goal)
- **Feature Calculation**: < 1ms
- **End-to-End Latency**: UDP → Signal < 5ms
- **Throughput**: > 100K updates/sec
- **Symbols**: Support 2000+ symbols

---

## Current Architecture Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                      FEEDER (Producer)                        │
│  ┌─────────┐  ┌─────────┐  ┌──────────┐  ┌─────────┐        │
│  │ Binance │  │  Bybit  │  │ Coinbase │  │   OKX   │        │
│  │ Builder │  │ Builder │  │ Builder  │  │ (books5)│        │
│  └────┬────┘  └────┬────┘  └────┬─────┘  └────┬────┘        │
│       │            │             │             │              │
│       └────────────┴─────────────┴─────────────┘              │
│                        │                                       │
│                   UDP Multicast                               │
│              (Port 9001 - Crypto Data)                        │
└───────────────────────┬──────────────────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────────────────┐
│                    CONSUMER (Receiver)                        │
│                                                                │
│  ┌──────────────────────────────────────────────────────┐    │
│  │            UDP Receiver (udp-protocol)               │    │
│  │   - Parses binary protocol                           │    │
│  │   - Sequence tracking                                │    │
│  │   - Outputs: OrderBookUpdate, Trade                  │    │
│  └─────────────────┬────────────────────────────────────┘    │
│                    │                                           │
│                    ▼                                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │       HFT OrderBook Processor (matching-engine)      │    │
│  │   - Lock-free ring buffers (65K capacity)            │    │
│  │   - Batch processing (16 updates/lock)               │    │
│  │   - Cache-optimized orderbooks                       │    │
│  │   - Integer-only arithmetic                          │    │
│  │   - CPU core pinning                                 │    │
│  │   - Supports 2048 symbols                            │    │
│  └─────────────────┬────────────────────────────────────┘    │
│                    │                                           │
│                    ▼                                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │    Feature Engineering (feature-engineering)         │    │
│  │   - WAP (0ms, 100ms, 200ms, 300ms)                   │    │
│  │   - OFI, TFI (Order/Trade Flow Imbalance)            │    │
│  │   - Order book imbalance metrics                     │    │
│  │   - Spread, volume at depth                          │    │
│  │   - Cross-exchange NBBO                              │    │
│  └─────────────────┬────────────────────────────────────┘    │
│                    │                                           │
│                    ▼                                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │      Signal Generation (signal-generation)           │    │
│  │   - Predict 15-second forward price                  │    │
│  │   - Risk-adjusted signals                            │    │
│  │   - Position sizing                                  │    │
│  └─────────────────┬────────────────────────────────────┘    │
│                    │                                           │
│                    ▼                                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │          Order Management System (oms)               │    │
│  │   - Order routing                                    │    │
│  │   - Risk management                                  │    │
│  │   - Position tracking                                │    │
│  └──────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

---

## Latest Implementation Status (2025-10-18)

### ✅ Completed: Consumer Binary Implementation

**Files Created**:
1. `crates/matching-engine/src/bin/consumer.rs` - Main consumer binary (230 lines)
2. `config/consumer/symbols.json` - Symbol configuration file

**Files Modified**:
1. `crates/matching-engine/Cargo.toml` - Added consumer binary and dependencies

**Implementation Details**:

The consumer binary implements the complete UDP → HFT OrderBook pipeline:

```rust
// Main data flow:
1. Load configuration from config/consumer/symbols.json
2. Create HFT OrderBook Processor
3. Register symbols from config (exchange-aware)
4. Start processing thread (CPU core pinning on Linux)
5. Create UDP Receiver for configured exchanges
6. Main event loop:
   - Receive OrderBookUpdate from UDP
   - Convert to FastUpdate (integer representation)
   - Push to exchange-specific ring buffer
   - Processing thread applies updates in batches
```

**Configuration System**:
```json
{
  "udp_config": {
    "port": 9001,
    "buffer_size": 10000
  },
  "symbols": [
    {
      "exchange": "Binance",
      "symbol": "BTC-USDT",
      "tick_size": 0.01,
      "enabled": true
    }
  ],
  "processing_config": {
    "cpu_core": 2,
    "ring_buffer_size": 65536,
    "batch_size": 16,
    "max_symbols": 2048
  }
}
```

**Features Implemented**:
- ✅ Configuration-driven symbol registration
- ✅ Multi-exchange support (Binance, Bybit, Coinbase, OKX, Deribit)
- ✅ Lock-free ring buffer integration
- ✅ Stats logging (orderbooks/sec, trades/sec)
- ✅ Environment variable config override (`CONSUMER_CONFIG`)
- ✅ Graceful error handling with anyhow

**Compilation Status**: ✅ **SUCCESS**
```
Finished `dev` profile [optimized + debuginfo] target(s) in 6.04s
```

---

## Next Steps

### Immediate (Phase 1, Day 1-2 completion):
1. **Test with live feeder**: Run feeder_direct and consumer together
2. **Verify ring buffer flow**: Check that updates reach HFT processor
3. **Monitor performance**: Verify sub-100µs orderbook update latency

### Short-term (Phase 1, Day 3-4):
1. **Feature Engineering Integration**:
   - Create feature engine module that reads from HFT processor
   - Implement periodic feature calculation (e.g., every 100ms)
   - Export features for signal generation

### Medium-term (Phase 1, Day 5-7):
1. **Cross-Exchange NBBO**:
   - Add NBBO calculation in feature engine
   - Compare best bid/ask across exchanges
   - Use for arbitrage signal generation

---

## Running the Consumer

```bash
# Development mode (default config)
cargo run --bin consumer

# With custom config
CONSUMER_CONFIG=config/consumer/symbols_custom.json cargo run --bin consumer

# Production mode
cargo build --release --bin consumer
./target/release/consumer

# With RUST_LOG for debugging
RUST_LOG=consumer=debug,matching_engine=debug cargo run --bin consumer
```

---

## Next Recommended Action

**Test the complete pipeline**:
1. In terminal 1: `cargo run --bin feeder_direct` (or `LIMITED_MODE=true` for limited symbols)
2. In terminal 2: `RUST_LOG=consumer=info cargo run --bin consumer`
3. Watch for:
   - Consumer logs showing orderbook updates received
   - Stats every 5 seconds: "X orderbooks/sec, Y trades/sec"
   - No "Ring buffer full" warnings

If the test succeeds, the UDP → HFT OrderBook pipeline is working end-to-end!
