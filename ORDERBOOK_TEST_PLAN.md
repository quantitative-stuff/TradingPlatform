# Orderbook Stream Testing Plan

## Objective
Empirically test what data format Upbit and Bithumb send with different subscription options.

## Test Setup

### Current Configuration Status:
1. ✅ **Upbit**: Changed to `isOnlySnapshot: true` (line 176, 183 in upbit.rs)
2. ⏸️ **Bithumb**: Uses default subscription (no snapshot/realtime option available)

## Test Procedure

### Test 1: Upbit with isOnlySnapshot=true ⏳ IN PROGRESS

**Command:**
```bash
RUST_LOG=info USE_LIMITED_CONFIG=1 timeout 30 ./target/release/feeder_direct.exe 2>&1 | tee logs/upbit_snapshot_test.log
```

**What to observe:**
- Frequency of orderbook updates
- `stream_type` field in Upbit messages (should see "SNAPSHOT" or "REALTIME")
- Number of `orderbook_units` in each message
- Whether consecutive messages show same or different prices/quantities

**Expected behavior (if truly snapshot-only):**
- Lower frequency updates (periodic snapshots)
- Each message contains FULL orderbook state
- Consecutive messages may have identical data if no changes

---

### Test 2: Upbit with isOnlyRealtime=true ⏸️ PENDING

**Configuration change needed:**
```rust
// In upbit.rs line 176 and 183, change:
"isOnlySnapshot": true  → "isOnlyRealtime": true
```

**Then recompile:**
```bash
cargo build --release --bin feeder_direct
```

**Command:**
```bash
RUST_LOG=info USE_LIMITED_CONFIG=1 timeout 30 ./target/release/feeder_direct.exe 2>&1 | tee logs/upbit_realtime_test.log
```

**What to observe:**
- Higher frequency updates (on every orderbook change)
- Whether data format differs from snapshot mode
- Whether it's still full snapshots or incremental deltas

---

### Test 3: Bithumb (default subscription)

**No code changes needed** - Bithumb should automatically send:
- Code 00006: Complete snapshot (one-time on subscribe)
- Code 00007: Incremental updates (ongoing)

**Command:**
```bash
RUST_LOG=info USE_LIMITED_CONFIG=1 timeout 30 ./target/release/feeder_direct.exe 2>&1 | grep -i bithumb | tee logs/bithumb_test.log
```

**What to observe:**
- Message codes (00006 vs 00007)
- `ver` field for version tracking
- Whether orderbook_units contains full state or just changes

---

## Analysis Questions

After running all tests, determine:

1. **Upbit isOnlySnapshot vs isOnlyRealtime:**
   - Do BOTH modes send full orderbook state?
   - Or does one send incremental deltas?
   - What is the update frequency difference?
   - Is there a `stream_type` field that differentiates them?

2. **Bithumb:**
   - Does it actually send code 00006/00007 distinctions?
   - Does code 00007 contain full state or incremental changes?
   - Is the `ver` field present and incrementing?

3. **Current implementation correctness:**
   - If both Upbit modes send full state → Current impl is CORRECT ✅
   - If one sends deltas → Need to handle differently ⚠️
   - If Bithumb code 00007 is incremental → Need to fix ⚠️

---

## Quick Manual Test Alternative

Instead of automated tests, you can:

1. Run feeder normally
2. Watch the console output for first 10 orderbook messages
3. Note down:
   - Message structure
   - Presence of sequence fields
   - Whether consecutive messages are identical or changing
   - Update frequency (high freq = realtime, low freq = periodic snapshot)

---

## Current Code Locations

- **Upbit subscription**: `crates/feeder/src/crypto/feed/upbit.rs` line 180-184
- **Upbit message processing**: `crates/feeder/src/crypto/feed/upbit.rs` line 418-489
- **Bithumb subscription**: `crates/feeder/src/crypto/feed/bithumb.rs` line 148-160
- **Bithumb message processing**: `crates/feeder/src/crypto/feed/bithumb.rs` line 320-398

---

## Expected Outcomes

Based on official docs, we expect:

**Upbit:**
- Both isOnlySnapshot and isOnlyRealtime send FULL orderbook state
- Difference is only UPDATE FREQUENCY
- No incremental deltas

**Bithumb:**
- Code 00006 = Full snapshot (one-time)
- Code 00007 = Incremental changes (ongoing) with `ver` field
- Current code treats all as snapshots → Potentially INCORRECT

**Recommendation:** Focus on testing Bithumb to confirm if code 00007 is truly incremental or still full snapshots.
