# Orderbook Implementation Review Results

## Summary

Reviewed all 7 exchanges for orderbook handling. Found 2 exchanges with critical issues.

---

## ✅ CORRECT IMPLEMENTATIONS

### 1. Binance - Incremental Only ✅
- **Pattern**: REST snapshot + WebSocket incremental
- **Implementation**: Correctly fetches REST snapshot, validates U/u/pu sequence
- **Status**: FIXED (just completed)

### 2. Coinbase - Snapshot + Incremental ✅
- **Pattern**: WebSocket sends type="snapshot" first, then type="update"
- **Implementation**: Maintains HashMap state, applies incremental updates correctly
- **Code**: Lines 499-572 in coinbase.rs
  - Gets/creates orderbook from HashMap
  - Applies updates (qty=0 = delete, otherwise update/add)
- **Status**: CORRECT

### 3. Deribit - Snapshot + Incremental ✅
- **Pattern**: type="snapshot" first, then type="change" with action codes
- **Implementation**: Maintains HashMap state, applies action-based updates
- **Code**: Lines 499-584 in deribit.rs
  - Handles "new", "change", "delete" actions correctly
  - Sorts orderbook after updates
- **Status**: CORRECT

### 4. Upbit - Unknown (Assumed Snapshot) ⚠️
- **Pattern**: isOnlyRealtime=true (unclear if snapshot or incremental)
- **Implementation**: Treats as full snapshots (direct replacement)
- **Status**: ASSUMED CORRECT (needs empirical verification if critical)

### 5. Bithumb - Unknown (Assumed Snapshot) ⚠️
- **Pattern**: Default subscription (unclear what it sends)
- **Implementation**: Treats as full snapshots (direct replacement)
- **Status**: ASSUMED CORRECT (needs empirical verification if critical)

---

## ❌ ISSUES FOUND

### 6. Bybit - Snapshot + Incremental ❌ WRONG

**Problem**: Treats delta updates as full snapshots

**Current Code (lines 544-598 in bybit.rs):**
```rust
match message_type {
    "snapshot" => {
        debug!("[Bybit] Processing snapshot");
    },
    "delta" => {
        debug!("[Bybit] Processing delta update");
        // ❌ PROBLEM: Just logs, doesn't maintain state!
    },
    _ => { ... }
}

// ❌ Both snapshot and delta create new OrderBookData
let orderbook = crate::core::OrderBookData { ... };
orderbooks.push(orderbook.clone());  // ❌ No state management!
sender.send_orderbook_data(orderbook);  // ❌ Sending deltas as full books!
```

**What it should do:**
- Maintain per-symbol orderbook state in HashMap
- For "snapshot": Replace entire orderbook
- For "delta": Apply incremental updates (qty=0 = delete)

**Impact**: Orderbook will be corrupted/incomplete for Bybit

---

### 7. OKX - Multiple Channels (Snapshot + Incremental) ⚠️ PARTIALLY WRONG

**Current Channel**: `books5` (snapshot-only, 5 levels) - CORRECT for this channel

**Problem**: Code also handles `books` and `books50-l2-tbt` channels, which are INCREMENTAL, but treats them as snapshots!

**Current Code (line 482 in okx.rs):**
```rust
"books5" | "books" | "books50-l2-tbt" => {
    // ❌ Treats ALL channels the same way (as snapshots)
    let orderbook = OrderBookData { ... };
    orderbooks.push(orderbook.clone());
    sender.send_orderbook_data(orderbook);
}
```

**Channel Types:**
- `books5`: Snapshot-only, 100ms, 5 levels ✅ Current impl is CORRECT for this
- `books`: **INCREMENTAL**, 100ms, 400 levels ❌ Should maintain state
- `books-l2-tbt`: **INCREMENTAL**, 10ms, 400 levels (VIP5+) ❌ Should maintain state
- `books50-l2-tbt`: **INCREMENTAL**, 10ms, 50 levels (VIP4+) ❌ Should maintain state

**Impact**:
- Current: Using `books5` = OK
- If switched to `books` or TBT channels = BROKEN

---

## Required Fixes

### Fix 1: Bybit ❌ CRITICAL
**Priority**: HIGH - Currently broken
**Effort**: Medium (15-20 minutes)

Need to:
1. Add HashMap to store orderbook state per symbol
2. For "snapshot": Replace entire book
3. For "delta": Apply incremental updates (qty=0 = delete)

### Fix 2: OKX ⚠️ MEDIUM PRIORITY
**Priority**: MEDIUM - Only broken if someone changes channel
**Effort**: Medium (15-20 minutes)

Options:
- **Option A**: Remove support for `books` and TBT channels (keep only `books5`)
- **Option B**: Add conditional logic to handle incremental channels differently
- **Option C**: Document that only `books5` is supported

---

## Recommendation

**Immediate Actions**:
1. **Fix Bybit NOW** - It's currently broken and we're using it
2. **Fix OKX** - Remove unsupported channels or add proper handling
3. **Document Upbit/Bithumb assumptions** - Note that we assume full snapshots

**Testing Priority**:
1. Test Bybit after fix
2. Verify OKX still works with `books5`
3. Empirically test Upbit/Bithumb if time permits

---

## Code Locations

- **Bybit**: `crates/feeder/src/crypto/feed/bybit.rs` lines 473-598
- **OKX**: `crates/feeder/src/crypto/feed/okx.rs` lines 482-574
- **Coinbase**: `crates/feeder/src/crypto/feed/coinbase.rs` lines 499-572 (reference for correct impl)
- **Deribit**: `crates/feeder/src/crypto/feed/deribit.rs` lines 499-584 (reference for correct impl)
