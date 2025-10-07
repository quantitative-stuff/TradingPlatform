# Multicast Fixes Applied - Summary

## Problem
Receiver was getting NO data from feeder across different machines.

## Root Causes Found
1. **Multi-port sender had TTL=1** → Could only work on same subnet
2. **Binary UDP sender was using `set_broadcast()`** → Wrong, should use multicast settings
3. **Only 3 exchanges were using multi_port_sender** → Other 4 were using wrong sender

## Fixes Applied ✅

### 1. Fixed Multi-Port UDP Sender TTL
**File:** `crates/feeder/src/core/multi_port_udp_sender.rs:105`
```rust
// Before:
socket2.set_multicast_ttl_v4(1)?;

// After:
socket2.set_multicast_ttl_v4(32)?; // Cross-machine capable
```

### 2. Fixed Binary UDP Sender
**File:** `crates/feeder/src/core/binary_udp_sender.rs:45-49`
```rust
// Before:
socket.set_broadcast(true)?;

// After:
socket.set_multicast_ttl_v4(32)?; // Cross-machine capable
socket.set_multicast_loop_v4(false)?;
```

### 3. Updated All Exchanges to Use Multi-Port Sender
Changed all 7 exchanges to use `get_multi_port_sender()` instead of `get_binary_udp_sender()`:

- ✅ **binance.rs** - Already using multi_port_sender
- ✅ **bybit.rs** - Already using multi_port_sender
- ✅ **okx.rs** - Already using multi_port_sender
- ✅ **upbit.rs** - FIXED: Now using multi_port_sender
- ✅ **deribit.rs** - FIXED: Now using multi_port_sender
- ✅ **coinbase.rs** - FIXED: Now using multi_port_sender
- ✅ **bithumb.rs** - FIXED: Now using multi_port_sender

## Multicast Configuration

### Sender (Feeder)
**Sends to 20 multicast addresses:**
```
Binance (8 addresses):
  - Trade: 239.10.1.1:9000-9003 (4 ports)
  - OrderBook: 239.10.1.1:9100-9103 (4 ports)

Bybit, OKX, Deribit, Upbit, Coinbase, Bithumb (2 addresses each):
  - Trade: 239.10.X.1:9000 (1 port)
  - OrderBook: 239.10.X.1:9100 (1 port)
```

### Receiver
**Listens on same 20 addresses:**
- Uses `MultiPortUdpReceiver`
- Binds to `0.0.0.0:<port>`
- Joins multicast groups via `join_multicast_v4()`

## Testing

### Build
```bash
cargo build --release --bin feeder_direct
```

### Run Sender (Feeder Machine)
```bash
cargo run --release --bin feeder_direct
```

**Look for in logs:**
```
✅ Multi-Port UDP sender initialized (20 addresses...)
✅ binance Trade pool: 239.10.1.1 ports 9000-9003
✅ binance OrderBook pool: 239.10.1.1 ports 9100-9103
...
```

### Run Receiver (Receiver Machine)
```bash
cargo run --release --bin multi_port_receiver
```

**Look for in logs:**
```
✅ Receiver binance_trade_0 listening on 239.10.1.1:9000
✅ Receiver binance_trade_1 listening on 239.10.1.1:9001
...
Received X bytes from binance (Trade) via binance_trade_0
Received X bytes from bybit (Trade) via bybit_trade_0
...
```

## If Still No Data

### 1. Firewall (Receiver Machine)
```powershell
# Windows (Run as Admin)
netsh advfirewall firewall add rule name="Multicast" dir=in action=allow protocol=UDP localport=9000-9103
```

```bash
# Linux
sudo iptables -A INPUT -p udp --dport 9000:9103 -j ACCEPT
sudo iptables -A INPUT -d 239.10.0.0/16 -j ACCEPT
```

### 2. Network Switch
- Must support **IGMP** (Internet Group Management Protocol)
- Must have **multicast forwarding** enabled
- Test: Connect machines directly with ethernet cable to bypass switch

### 3. Same Subnet Check
```bash
# Both machines must be on same subnet
# Example:
#   Sender: 192.168.1.10/24  ✅
#   Receiver: 192.168.1.20/24  ✅
```

### 4. Test on Same Machine First
```bash
# Terminal 1
cargo run --release --bin feeder_direct

# Terminal 2
cargo run --release --bin multi_port_receiver
```

If this works but cross-machine doesn't → Network issue (firewall/switch/router)

## Verification Commands

### Check if ports are bound:
```bash
# Windows
netstat -ano | findstr :9000

# Linux
netstat -tulpn | grep 9000
```

### Capture packets with Wireshark:
```
Display filter: ip.dst >= 239.10.1.1 and ip.dst <= 239.10.7.1
```

## Summary of Changes

**Files Modified:**
1. `crates/feeder/src/core/multi_port_udp_sender.rs` - TTL 1→32
2. `crates/feeder/src/core/binary_udp_sender.rs` - Fixed multicast config
3. `crates/feeder/src/crypto/feed/upbit.rs` - Use multi_port_sender
4. `crates/feeder/src/crypto/feed/deribit.rs` - Use multi_port_sender
5. `crates/feeder/src/crypto/feed/coinbase.rs` - Use multi_port_sender
6. `crates/feeder/src/crypto/feed/bithumb.rs` - Use multi_port_sender

**Build Status:** ✅ Compiled successfully

The receiver should now get data from all 7 exchanges.
