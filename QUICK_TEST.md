# Quick Test - What You Should See

## Build Both Binaries
```bash
cargo build --release --bin feeder_direct
cargo build --release --bin multi_port_receiver
```

## Run Tests

### Machine 1: Start Feeder
```bash
cargo run --release --bin feeder_direct
```

**Look for:** `✅ All 20 multicast addresses initialized`

### Machine 2: Start Receiver
```bash
cargo run --release --bin multi_port_receiver
```

## Critical Success Indicators

### 1. Receiver Startup (All 20 Must Appear):
```
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

### 2. First Packets Received (Should appear within 30 seconds):
```
DEBUG RECEIVER: binance_trade_0 received FIRST packet: 98 bytes from 192.168.X.X:XXXXX
DEBUG RECEIVER: binance_orderbook_0 received FIRST packet: 130 bytes from 192.168.X.X:XXXXX
DEBUG RECEIVER: bybit_trade_0 received FIRST packet: 98 bytes from 192.168.X.X:XXXXX
... (continue for all active exchanges)
```

### 3. Ongoing Traffic (Every 1000 packets):
```
DEBUG RECEIVER: binance_trade_0 received 1000 packets (0 MB total)
DEBUG RECEIVER: binance_trade_0 received 2000 packets (0 MB total)
DEBUG RECEIVER: binance_orderbook_0 received 1000 packets (0 MB total)
...
```

## If You See This - FAIL ❌

### All Joins, But NO First Packets:
```
✅ Started 20 receiver threads
(then nothing... no "received FIRST packet" messages)
```
**Problem:** Network issue - packets not reaching receiver
**Quick Fix:** Check firewall, run on same machine first

### Some First Packets, But Not All:
```
DEBUG RECEIVER: binance_trade_0 received FIRST packet ...
DEBUG RECEIVER: bybit_trade_0 received FIRST packet ...
(but missing okx, deribit, upbit, coinbase, bithumb)
```
**Problem:** Those exchanges may not be feeding yet, or sender not initialized for them
**Check:** Feeder logs should show connections for those exchanges

### Packet Counts Way Too Low:
```
Feeder shows: 10000 packets sent
Receiver shows: 500 packets received (only 5%)
```
**Problem:** Severe UDP packet loss (>95%)
**Cause:** Network congestion, buffer overflow, or firewall dropping packets

## Quick Diagnostics

### Check Receiver Listening:
```bash
# Windows
netstat -ano | findstr :9000

# Linux
netstat -tulpn | grep 9000
```

### Check Multicast Groups Joined:
```bash
# Windows (PowerShell as Admin)
netsh interface ipv4 show joins

# Linux
ip maddr show
```

Should see `239.10.1.1` through `239.10.7.1` in the output.

### Allow Firewall (Windows - Run as Admin):
```powershell
netsh advfirewall firewall add rule name="Multicast" dir=in action=allow protocol=UDP localport=9000-9103
```

### Allow Firewall (Linux):
```bash
sudo iptables -A INPUT -p udp --dport 9000:9103 -j ACCEPT
sudo iptables -A INPUT -d 239.10.0.0/16 -j ACCEPT
```

## Expected Packet Loss

**Normal UDP:** 1-5% packet loss is expected
**Acceptable:** Up to 10% if network is congested
**Problem:** >20% indicates serious issue (firewall, congestion, buffer overflow)

## Test on Same Machine First

If cross-machine fails, test on same machine:
```bash
# Terminal 1
cargo run --release --bin feeder_direct

# Terminal 2
cargo run --release --bin multi_port_receiver
```

If this works but cross-machine doesn't → Network issue (firewall/switch/router)
If this fails → Code issue (check logs carefully)
