# How to Check Orderbook Data

I've added debug logging that will show you exactly what Upbit and Bithumb are sending!

## Step 1: Run the feeder

**Stop any running feeder first**, then run:

```bash
RUST_LOG=info USE_LIMITED_CONFIG=1 ./target/release/feeder_direct.exe 2>&1 | tee logs/orderbook_test.log
```

## Step 2: Watch for the 🔬 debug messages

You'll see lines like this:

```
🔬 Upbit orderbook: stream_type=SNAPSHOT, timestamp=1697123456789, units=15
🔬 Upbit orderbook: stream_type=REALTIME, timestamp=1697123456890, units=15
🔬 Bithumb orderbook: code=KRW-BTC, ver=12345, units=10
```

## Step 3: What to observe

### For UPBIT:

**Key questions:**
1. **stream_type field**: Is it "SNAPSHOT", "REALTIME", or "NONE"?
2. **Update frequency**: Count how many messages per second
   - Snapshot mode = low frequency (1-2 per second)
   - Realtime mode = high frequency (10+ per second)
3. **Timestamp changes**: Are consecutive messages very close in time?

**Example output analysis:**
```
🔬 Upbit orderbook: stream_type=SNAPSHOT, timestamp=1000, units=15
🔬 Upbit orderbook: stream_type=SNAPSHOT, timestamp=1500, units=15  (500ms gap)
🔬 Upbit orderbook: stream_type=SNAPSHOT, timestamp=2000, units=15  (500ms gap)
```
☝️ This shows periodic snapshots every 500ms

```
🔬 Upbit orderbook: stream_type=REALTIME, timestamp=1000, units=15
🔬 Upbit orderbook: stream_type=REALTIME, timestamp=1050, units=15  (50ms gap)
🔬 Upbit orderbook: stream_type=REALTIME, timestamp=1120, units=15  (70ms gap)
```
☝️ This shows frequent updates on every change

### For BITHUMB:

**Key questions:**
1. **code field**: Does it show "00006" or "00007" (or just symbol name)?
2. **ver field**: Is it present and incrementing?
   - ver=100, ver=101, ver=102 → incremental with version tracking
   - ver=0, ver=0, ver=0 → no version tracking
3. **Units count**: Always 10? Or varying?

## Step 4: Let it run for 30 seconds

Watch the pattern for about 30 seconds, then press Ctrl+C to stop.

## Step 5: Analyze the log

```bash
# Count Upbit messages
grep "🔬 Upbit orderbook" logs/orderbook_test.log | wc -l

# Check Upbit stream types
grep "🔬 Upbit orderbook" logs/orderbook_test.log | head -20

# Count Bithumb messages
grep "🔬 Bithumb orderbook" logs/orderbook_test.log | wc -l

# Check Bithumb codes and versions
grep "🔬 Bithumb orderbook" logs/orderbook_test.log | head -20
```

## Expected Results

### Test 1: isOnlySnapshot=true (CURRENT)

**Upbit:**
- stream_type = "SNAPSHOT" (or possibly "NONE")
- Low frequency (~1-2 per second)
- Each message contains full orderbook state

**Bithumb:**
- Should show actual message structure
- Check if code field shows 00006/00007

### Test 2: Change to isOnlyRealtime=true

Change line 176 and 183 in `upbit.rs`:
```rust
"isOnlySnapshot": true  →  "isOnlyRealtime": true
```

Recompile and run again. Compare:

**Upbit:**
- stream_type = "REALTIME" (or possibly "NONE")
- High frequency (10+ per second)
- Each message still contains full orderbook state (hypothesis)

## Quick Analysis Script

After running the test, use this:

```bash
echo "=== UPBIT ANALYSIS ==="
echo "Total messages: $(grep -c "🔬 Upbit orderbook" logs/orderbook_test.log)"
echo "Stream types: $(grep "🔬 Upbit orderbook" logs/orderbook_test.log | head -10)"
echo ""
echo "=== BITHUMB ANALYSIS ==="
echo "Total messages: $(grep -c "🔬 Bithumb orderbook" logs/orderbook_test.log)"
echo "Codes/Versions: $(grep "🔬 Bithumb orderbook" logs/orderbook_test.log | head -10)"
```

## What We're Testing

**Hypothesis:**
- Upbit: BOTH snapshot and realtime send FULL orderbook (just different frequency)
- Bithumb: Might send incremental updates with version tracking

**This test will confirm or disprove our hypothesis!**
