# Exchange Orderbook Patterns & Implementation Plan

## Executive Summary

Comprehensive review of all 7 exchanges' orderbook feeding patterns and implementation plan following `local_orderbook_builder_plan.md` guidelines.

---

## Exchange Orderbook Patterns

### 1. Binance - REST Snapshot + WebSocket Incremental ✅ IMPLEMENTED
**Pattern**: Hybrid approach requiring synchronization
- **Initial State**: Fetch REST snapshot from `/api/v3/depth?symbol=X&limit=5000`
- **Updates**: WebSocket `@depth` stream sends incremental updates
- **Synchronization**: Buffer WebSocket events, discard events with `u <= lastUpdateId`, validate first event overlaps
- **Current Status**: ✅ Correctly implemented in `crates/feeder/src/crypto/feed/binance_snapshot.rs`
- **Architecture**: Uses `BinanceOrderbookSynchronizer` per symbol with event buffering

**What it does**:
```
1. Subscribe to WebSocket @depth stream
2. Buffer incoming events
3. Fetch REST snapshot (get lastUpdateId)
4. Drop buffered events where u <= lastUpdateId
5. Apply remaining buffered events
6. Continue with live WebSocket stream
```

---

### 2. Bybit - WebSocket Snapshot + Delta ❌ NEEDS FIXING (Currently uses RwLock shortcut)
**Pattern**: WebSocket-only with snapshot + incremental
- **Initial State**: WebSocket sends `type="snapshot"` on subscription
- **Updates**: WebSocket sends `type="delta"` messages
- **Delta Logic**: qty=0 means delete level, otherwise update/insert
- **Current Problem**: Currently implemented with RwLock (violates lock-free architecture from plan)
- **Required Fix**: Implement per-symbol task architecture with mpsc channels

**What it should do**:
```
1. Subscribe to WebSocket orderbook.{depth}.{symbol}
2. Receive snapshot message → reset local orderbook
3. Receive delta messages → apply incremental updates
   - qty=0: delete price level
   - qty>0: update or insert price level
```

---

### 3. Coinbase - Snapshot + Incremental ✅ CORRECT (Already uses HashMap state)
**Pattern**: WebSocket snapshot + updates
- **Initial State**: WebSocket sends `type="snapshot"` on subscription
- **Updates**: WebSocket sends `type="update"` messages
- **Update Logic**: new_quantity=0 means delete level
- **Current Status**: ✅ Already maintains HashMap state correctly
- **Location**: `crates/feeder/src/crypto/feed/coinbase.rs` lines 499-572
- **Architecture**: Uses `orderbooks: HashMap` with proper state management

---

### 4. Deribit - Snapshot + Incremental ✅ CORRECT (Already uses HashMap state)
**Pattern**: WebSocket snapshot + action-based updates
- **Initial State**: WebSocket sends `type="snapshot"` on subscription
- **Updates**: WebSocket sends `type="change"` with action codes
- **Actions**: "new", "change", "delete"
- **Current Status**: ✅ Already maintains HashMap state correctly
- **Location**: `crates/feeder/src/crypto/feed/deribit.rs` lines 499-584
- **Architecture**: Uses `orderbooks: HashMap` with proper state management

---

### 5. OKX - Multiple Channels (Currently books5) ⚠️ PARTIALLY WRONG
**Pattern**: Different channels have different patterns

| Channel | Type | Update Frequency | Depth | VIP Required | Current Support |
|---------|------|------------------|-------|--------------|-----------------|
| books5 | SNAPSHOT | 100ms | 5 | No | ✅ CORRECT |
| books | INCREMENTAL | 100ms | 400 | No | ❌ WRONG (treats as snapshot) |
| books-l2-tbt | INCREMENTAL | 10ms | 400 | VIP5+ | ❌ WRONG (treats as snapshot) |
| books50-l2-tbt | INCREMENTAL | 10ms | 50 | VIP4+ | ❌ WRONG (treats as snapshot) |

**Current Problem**: Code at line 482 handles all channels the same way (as snapshots)

```rust
"books5" | "books" | "books50-l2-tbt" => {
    // ❌ Treats ALL channels the same way (as snapshots)
    let orderbook = OrderBookData { /* ... */ };
}
```

**Required Fix**: Either:
- **Option A**: Remove support for incremental channels (only keep books5)
- **Option B**: Implement per-symbol task architecture for incremental channels

**Recommendation**: Option A - Remove support for `books`, `books-l2-tbt`, `books50-l2-tbt` since we're using `books5`

---

### 6. Upbit - Unknown (Assumed Snapshot) ⚠️
**Pattern**: Unclear, likely snapshot-only
- **Subscription**: Uses `isOnlyRealtime: true`
- **Current Assumption**: Treats as full snapshots
- **Current Status**: ⚠️ Assumed correct but unverified
- **Risk**: Low (if assumption wrong, orderbook will be corrupt but won't crash)

---

### 7. Bithumb - Unknown (Assumed Snapshot) ⚠️
**Pattern**: Unclear, likely snapshot-only
- **Subscription**: Default orderbook subscription
- **Current Assumption**: Treats as full snapshots
- **Current Status**: ⚠️ Assumed correct but unverified
- **Risk**: Low (if assumption wrong, orderbook will be corrupt but won't crash)

---

## Implementation Plan (Following local_orderbook_builder_plan.md)

### Phase 1: Per-Symbol Task Architecture (Critical)

**Goal**: Implement lock-free per-symbol orderbook processing using mpsc channels

**Pattern from plan** (Step 6, lines 244-277):
```rust
pub struct SymbolManager {
    builders: HashMap<String, tokio::task::JoinHandle<()>>,
    senders: HashMap<String, mpsc::Sender<OrderBookUpdate>>,
}

// For each symbol:
1. Spawn dedicated task
2. Create mpsc channel
3. Send updates via channel
4. Task processes updates sequentially (NO LOCKS)
```

**Benefits**:
- ✅ No lock contention (each symbol isolated)
- ✅ Sequential processing per symbol (no race conditions)
- ✅ Better scaling (1000+ symbols)
- ✅ Simpler reasoning (each symbol = independent state machine)

---

### Implementation Steps

#### Step 1: Bybit - Implement Per-Symbol Task Architecture ⏭️ NEXT
**Current**: Uses `Arc<parking_lot::RwLock<HashMap<String, OrderBookData>>>` (WRONG - lock-based)
**Target**: Use `HashMap<String, mpsc::Sender<OrderBookUpdate>>` (CORRECT - lock-free)

**Changes required**:
1. Define `OrderBookUpdate` message type
2. Create `HashMap<String, mpsc::Sender<OrderBookUpdate>>` in struct
3. In `new()`: For each symbol, spawn task + create channel
4. In `process_bybit_message()`: Send updates to symbol's channel (no locks)
5. Each symbol's task: Maintain local `OrderBookData` state, apply updates sequentially

**Files to modify**:
- `crates/feeder/src/crypto/feed/bybit.rs`

---

#### Step 2: Coinbase & Deribit - Verify Existing Implementation ✅
**Status**: Already correct (use HashMap state)
**Action**: Review code to ensure it follows best practices
- Verify no deadlocks possible
- Confirm proper error handling
- Check for potential race conditions

**Files to review**:
- `crates/feeder/src/crypto/feed/coinbase.rs`
- `crates/feeder/src/crypto/feed/deribit.rs`

---

#### Step 3: OKX - Remove Incremental Channel Support
**Current**: Subscribes to `books5` (correct) but code handles all channels
**Target**: Remove handling for `books`, `books-l2-tbt`, `books50-l2-tbt`

**Changes required**:
1. Update subscription logic to ONLY allow `books5`
2. Remove channel handling for incremental channels
3. Add error/warning if config tries to use incremental channels
4. Document that only `books5` is supported

**Files to modify**:
- `crates/feeder/src/crypto/feed/okx.rs`

---

#### Step 4: Binance - Already Implemented Correctly ✅
**Status**: Already follows proper architecture
**Implementation**: Uses `BinanceOrderbookSynchronizer` with buffering and sequence validation
**Action**: No changes needed

---

#### Step 5: Upbit & Bithumb - Document Assumptions ⚠️
**Current**: Assumes snapshot-only (direct replacement)
**Action**: Add documentation and logging
- Document that we assume full snapshots
- Add debug logging to verify assumption empirically
- If found to be incremental, implement proper handling later

**Files to modify**:
- `crates/feeder/src/crypto/feed/upbit.rs` (add comment)
- `crates/feeder/src/crypto/feed/bithumb.rs` (add comment)

---

## Summary Table

| Exchange | Pattern | Current Status | Action Required |
|----------|---------|----------------|-----------------|
| Binance | REST snapshot + WS incremental | ✅ CORRECT | None |
| Bybit | WS snapshot + delta | ❌ WRONG (uses RwLock) | Implement per-symbol tasks |
| Coinbase | WS snapshot + incremental | ✅ CORRECT | Review only |
| Deribit | WS snapshot + incremental | ✅ CORRECT | Review only |
| OKX | Snapshot-only (books5) | ⚠️ PARTIAL (handles wrong channels) | Remove incremental support |
| Upbit | Unknown (assume snapshot) | ⚠️ UNVERIFIED | Document assumption |
| Bithumb | Unknown (assume snapshot) | ⚠️ UNVERIFIED | Document assumption |

---

## Priority Order

1. **🔴 CRITICAL**: Bybit - Implement per-symbol task architecture (currently broken with RwLock)
2. **🟡 MEDIUM**: OKX - Remove incremental channel handling
3. **🟢 LOW**: Upbit/Bithumb - Document assumptions

---

## Testing Plan

After implementation:
1. Test Bybit with live data - verify snapshot + delta handling
2. Test OKX with books5 - verify snapshot-only behavior
3. Monitor Upbit/Bithumb empirically - check for signs of incremental updates
4. Load test with 1000+ symbols - verify no lock contention

---

## Notes

- **NO SHORTCUTS**: Must follow local_orderbook_builder_plan.md
- **NO RwLock**: Per-symbol tasks with mpsc channels only
- **NO f64**: Continue using scaled i64 for HFT performance
- All changes must maintain existing UDP multicast output
