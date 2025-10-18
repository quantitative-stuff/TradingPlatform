# Orderbook Implementation Status

## Summary

After reviewing all exchange implementations and official documentation:

### ✅ Correctly Implemented

1. **Binance** - Incremental only (REST snapshot + WebSocket sync) ✅ FIXED
   - Fetches REST snapshot
   - Validates WebSocket sequence (U/u/pu)
   - Applies incremental updates correctly

2. **OKX** - Snapshot only (books5 channel) ✅
   - Each message is full snapshot
   - Direct replacement - correct approach

### ⚠️ NEEDS REVIEW/FIXING

3. **Upbit** - Unknown if snapshot or incremental ⚠️
   - Uses `isOnlyRealtime: true`
   - Current code: Treats as full snapshots (replace)
   - **Unknown**: Does "realtime" send full snapshots or deltas?
   - **Current assumption**: Full snapshots (probably correct)

4. **Bithumb** - Unknown if snapshot or incremental ⚠️
   - Current code: Treats as full snapshots (replace)
   - **Unknown**: What does default subscription actually send?
   - **Current assumption**: Full snapshots (probably correct)

5. **Bybit** - Snapshot + Incremental ❌ WRONG
   - **Receives**: type="snapshot" first, then type="delta"
   - **Current code**: Treats deltas as full snapshots ❌
   - **Problem**: Not maintaining local state for incremental updates
   - **Fix needed**: Maintain orderbook state, apply delta updates

6. **Coinbase** - Snapshot + Incremental ❓ NEEDS REVIEW
   - **Pattern**: type="snapshot" first, then type="update"
   - Need to check if current code handles this correctly

7. **Deribit** - Snapshot + Incremental ❓ NEEDS REVIEW
   - **Pattern**: type="snapshot" first, then type="change" with actions
   - Need to check if current code handles this correctly

---

## Critical Issue: Bybit

**Current Code (lines 544-598 in bybit.rs):**
```rust
match message_type {
    "snapshot" => {
        debug!("[Bybit] Processing snapshot for {}", original_symbol);
        // For snapshots, use all data as-is
    },
    "delta" => {
        debug!("[Bybit] Processing delta update for {} (bids: {}, asks: {})",
            original_symbol, bids.len(), asks.len());
        // For deltas, we still pass all data but log the distinction
        // Downstream consumers can handle quantity=0 as deletion if needed
    },
    _ => { ... }
}

// ❌ PROBLEM: Both snapshot and delta create new OrderBookData and send it
let orderbook = crate::core::OrderBookData { ... };
orderbooks.push(orderbook.clone());  // ❌ No state management!
sender.send_orderbook_data(orderbook);  // ❌ Sending deltas as full books!
```

**What It Should Do:**
```rust
// Maintain per-symbol orderbook state
static BYBIT_ORDERBOOKS: Lazy<RwLock<HashMap<String, OrderBookState>>> = ...;

match message_type {
    "snapshot" => {
        // Replace entire orderbook
        let orderbook = OrderBookState::from_snapshot(bids, asks);
        BYBIT_ORDERBOOKS.write().insert(symbol, orderbook);
    },
    "delta" => {
        // Apply incremental updates
        if let Some(orderbook) = BYBIT_ORDERBOOKS.write().get_mut(&symbol) {
            for (price, qty) in bids {
                if qty == 0 {
                    orderbook.remove_bid(price);  // Delete level
                } else {
                    orderbook.update_bid(price, qty);  // Update/insert
                }
            }
            // Same for asks...
        }
    }
}
```

---

## Recommendation

**Option 1: Fix all exchanges properly (Professional approach)**
- Fix Bybit to maintain state for deltas
- Review and fix Coinbase if needed
- Review and fix Deribit if needed
- Test empirically for Upbit/Bithumb to confirm they send full snapshots

**Option 2: Document assumptions and fix later**
- Document that Upbit/Bithumb assume full snapshots
- Mark Bybit/Coinbase/Deribit as "known issues"
- Fix when integrating with KDB+ where data quality matters

**Option 3: Quick fix for Bybit only**
- Since Bybit is confirmed to use snapshot+delta pattern
- Fix Bybit state management now
- Leave others for later review

**My recommendation: Option 3** - Fix Bybit since we know it's wrong, leave Upbit/Bithumb as-is (likely correct), review Coinbase/Deribit next.
