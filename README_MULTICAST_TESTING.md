# Multicast UDP System - Ready for Testing

## Status: ✅ READY TO TEST

Both binaries compiled successfully with all fixes applied.

## What Was Fixed

### 1. Multi-Port Sender TTL (Cross-Machine Support)
- **File:** `multi_port_udp_sender.rs:95`
- **Change:** TTL 1 → 32 (enables cross-machine multicast)

### 2. Binary UDP Sender Multicast Config
- **File:** `binary_udp_sender.rs:45-49`
- **Change:** Removed `set_broadcast()`, added proper multicast settings

### 3. All Exchanges Using Multi-Port Sender
- **Files:** `upbit.rs`, `deribit.rs`, `coinbase.rs`, `bithumb.rs`
- **Change:** Switched from `get_binary_udp_sender()` to `get_multi_port_sender()`

### 4. Receiver Debug Output
- **File:** `multi_port_udp_receiver.rs`
- **Added:** Debug logs for multicast joins and packet reception tracking

## Quick Start

### Build
```bash
cd D:\github\TradingPlatform
cargo build --release --bin feeder_direct
cargo build --release --bin multi_port_receiver
```

### Run Sender (Machine 1)
```bash
cargo run --release --bin feeder_direct
```

**Expected output:**
```
✅ Binary UDP sender initialized (66-byte headers + binary payload)
Initializing Multi-Port UDP Sender (Proportional)
Address allocation (20 total):
  Binance  → 8 addresses (4 trade + 4 orderbook)
  Others   → 2 addresses each (1 trade + 1 orderbook)
✅ All 20 multicast addresses initialized
✅ Multi-Port UDP sender initialized (20 addresses: Binance 40%, others 10% each)
```

### Run Receiver (Machine 2)
```bash
cargo run --release --bin multi_port_receiver
```

**Expected output (must see ALL 20):**
```
Starting Multi-Port UDP Receiver (20 addresses)
DEBUG RECEIVER: binance_trade_0 successfully joined multicast group 239.10.1.1:9000
DEBUG RECEIVER: binance_trade_1 successfully joined multicast group 239.10.1.1:9001
DEBUG RECEIVER: binance_trade_2 successfully joined multicast group 239.10.1.1:9002
DEBUG RECEIVER: binance_trade_3 successfully joined multicast group 239.10.1.1:9003
DEBUG RECEIVER: binance_orderbook_0 successfully joined multicast group 239.10.1.1:9100
DEBUG RECEIVER: binance_orderbook_1 successfully joined multicast group 239.10.1.1:9101
DEBUG RECEIVER: binance_orderbook_2 successfully joined multicast group 239.10.1.1:9102
DEBUG RECEIVER: binance_orderbook_3 successfully joined multicast group 239.10.1.1:9103
DEBUG RECEIVER: bybit_trade_0 successfully joined multicast group 239.10.2.1:9000
DEBUG RECEIVER: bybit_orderbook_0 successfully joined multicast group 239.10.2.1:9100
DEBUG RECEIVER: okx_trade_0 successfully joined multicast group 239.10.3.1:9000
DEBUG RECEIVER: okx_orderbook_0 successfully joined multicast group 239.10.3.1:9100
DEBUG RECEIVER: deribit_trade_0 successfully joined multicast group 239.10.4.1:9000
DEBUG RECEIVER: deribit_orderbook_0 successfully joined multicast group 239.10.4.1:9100
DEBUG RECEIVER: upbit_trade_0 successfully joined multicast group 239.10.5.1:9000
DEBUG RECEIVER: upbit_orderbook_0 successfully joined multicast group 239.10.5.1:9100
DEBUG RECEIVER: coinbase_trade_0 successfully joined multicast group 239.10.6.1:9000
DEBUG RECEIVER: coinbase_orderbook_0 successfully joined multicast group 239.10.6.1:9100
DEBUG RECEIVER: bithumb_trade_0 successfully joined multicast group 239.10.7.1:9000
DEBUG RECEIVER: bithumb_orderbook_0 successfully joined multicast group 239.10.7.1:9100
✅ Started 20 receiver threads
```

**When data flows (within 30 seconds):**
```
DEBUG RECEIVER: binance_trade_0 received FIRST packet: 98 bytes from 192.168.X.X:XXXXX
DEBUG RECEIVER: binance_orderbook_0 received FIRST packet: 130 bytes from 192.168.X.X:XXXXX
DEBUG RECEIVER: bybit_trade_0 received FIRST packet: 98 bytes from 192.168.X.X:XXXXX
... (and so on for each active exchange)
```

**Ongoing (every 1000 packets):**
```
DEBUG RECEIVER: binance_trade_0 received 1000 packets (0 MB total)
DEBUG RECEIVER: binance_trade_0 received 2000 packets (0 MB total)
...
```

## Multicast Architecture

### 20 Multicast Addresses:
```
Exchange    | Multicast IP  | Trade Ports | OrderBook Ports | Total
------------|---------------|-------------|-----------------|-------
Binance     | 239.10.1.1    | 9000-9003   | 9100-9103       | 8
Bybit       | 239.10.2.1    | 9000        | 9100            | 2
OKX         | 239.10.3.1    | 9000        | 9100            | 2
Deribit     | 239.10.4.1    | 9000        | 9100            | 2
Upbit       | 239.10.5.1    | 9000        | 9100            | 2
Coinbase    | 239.10.6.1    | 9000        | 9100            | 2
Bithumb     | 239.10.7.1    | 9000        | 9100            | 2
------------|---------------|-------------|-----------------|-------
TOTAL       |               |             |                 | 20
```

## Troubleshooting

### Receiver Joins But No Data

**Symptoms:**
```
✅ Started 20 receiver threads
(but no "received FIRST packet" messages)
```

**Diagnosis:** Network issue - packets not reaching receiver

**Solutions:**
1. **Test on same machine first:**
   ```bash
   # Terminal 1
   cargo run --release --bin feeder_direct

   # Terminal 2
   cargo run --release --bin multi_port_receiver
   ```
   If this works → Network issue (firewall/switch)
   If this fails → Code issue (check logs)

2. **Check firewall (Windows - Run as Admin):**
   ```powershell
   netsh advfirewall firewall add rule name="Multicast" dir=in action=allow protocol=UDP localport=9000-9103
   ```

3. **Check firewall (Linux):**
   ```bash
   sudo iptables -A INPUT -p udp --dport 9000:9103 -j ACCEPT
   sudo iptables -A INPUT -d 239.10.0.0/16 -j ACCEPT
   ```

4. **Verify multicast groups joined (Windows):**
   ```powershell
   netsh interface ipv4 show joins
   ```
   Should see `239.10.1.1` through `239.10.7.1`

5. **Verify multicast groups joined (Linux):**
   ```bash
   ip maddr show
   ```

6. **Check network switch:**
   - Must support IGMP (Internet Group Management Protocol)
   - Must have multicast forwarding enabled
   - Test with direct ethernet cable if unsure

### High Packet Loss (>20%)

**Normal UDP loss:** 1-5%
**Acceptable:** Up to 10% under load
**Problem:** >20% indicates serious issue

**Solutions:**
1. Increase receiver buffers (edit `multi_port_udp_receiver.rs:126`):
   ```rust
   socket2.set_recv_buffer_size(8 * 1024 * 1024)?; // 8MB
   ```

2. Increase sender buffers (edit `multi_port_udp_sender.rs:94`):
   ```rust
   socket2.set_send_buffer_size(4 * 1024 * 1024)?; // 4MB
   ```

3. Check network congestion with Wireshark:
   ```
   Filter: ip.dst >= 239.10.1.1 and ip.dst <= 239.10.7.1
   ```

### Some Exchanges Not Sending

**Symptoms:**
```
DEBUG RECEIVER: binance_trade_0 received FIRST packet ...
DEBUG RECEIVER: bybit_trade_0 received FIRST packet ...
(but missing okx, deribit, upbit, coinbase, bithumb)
```

**Diagnosis:** Those exchanges may not have active feeds yet

**Check:** Feeder logs should show WebSocket connections for those exchanges

## Packet Count Differences

**Question:** Should sender and receiver packet counts match exactly?

**Answer:** No, UDP is lossy and there are other factors:

1. **UDP Packet Loss (Normal):** 1-5% expected
2. **Timing:** Receiver started after sender already sent packets
3. **OrderBook Split:** Each orderbook becomes 2 packets (bids + asks)
4. **Network Congestion:** Temporary packet loss during high load

**Example:**
```
Sender: 10000 packets sent
Receiver: 9500 packets received (95% delivery = 5% loss)
This is NORMAL for UDP
```

**When to worry:**
- **>20% loss:** Network congestion or buffer overflow
- **>50% loss:** Serious network issue (firewall, switch, routing)
- **0% received:** Configuration error (multicast not working)

## Network Requirements

### Same Subnet (Simplest):
```
Sender:   192.168.1.10/24
Receiver: 192.168.1.20/24
✅ Works with TTL=32
```

### Different Subnets:
```
Sender:   192.168.1.10/24
Receiver: 192.168.2.20/24
⚠️ Requires multicast-capable router with IGMP
```

### Switch Requirements:
- IGMP support
- Multicast forwarding enabled
- If unsure → Test with direct ethernet cable

## Verification Commands

### Check ports bound:
```bash
# Windows
netstat -ano | findstr :9000

# Linux
netstat -tulpn | grep 9000
```

### Capture multicast packets:
```bash
# Linux
sudo tcpdump -i any -n 'dst net 239.10.0.0/16'

# Windows (Wireshark)
Display filter: ip.dst >= 239.10.1.1 and ip.dst <= 239.10.7.1
```

## Files Modified

1. `crates/feeder/src/core/multi_port_udp_sender.rs` - TTL fix
2. `crates/feeder/src/core/binary_udp_sender.rs` - Multicast config fix
3. `crates/feeder/src/core/multi_port_udp_receiver.rs` - Debug output added
4. `crates/feeder/src/crypto/feed/upbit.rs` - Use multi_port_sender
5. `crates/feeder/src/crypto/feed/deribit.rs` - Use multi_port_sender
6. `crates/feeder/src/crypto/feed/coinbase.rs` - Use multi_port_sender
7. `crates/feeder/src/crypto/feed/bithumb.rs` - Use multi_port_sender

## Documentation

- `FIXES_APPLIED.md` - Complete summary of all fixes
- `TESTING_GUIDE.md` - Comprehensive testing instructions
- `QUICK_TEST.md` - Quick reference for testing
- `DEBUG_NO_DATA.md` - Troubleshooting guide
- `diagnose_multicast.md` - Network diagnostics

## Next Steps

1. **Run feeder on Machine 1**
2. **Run receiver on Machine 2**
3. **Verify all 20 receivers join multicast groups**
4. **Verify first packets received within 30 seconds**
5. **Monitor packet counts and check loss percentage**
6. **If issues:** Follow troubleshooting section above

The system is now ready for cross-machine testing with full debug visibility.
