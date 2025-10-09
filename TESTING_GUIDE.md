# Multicast UDP Testing Guide

## Build Status ✅
Both binaries compiled successfully:
- `feeder_direct` - Sends to 20 multicast addresses
- `multi_port_receiver` - Receives from 20 multicast addresses

## Test Setup

### Machine 1: Feeder (Sender)
```bash
cd D:\github\TradingPlatform
cargo run --release --bin feeder_direct
```

### Machine 2: Receiver
```bash
cd D:\github\TradingPlatform
cargo run --release --bin multi_port_receiver
```

## Expected Output

### Feeder Startup (Look for these lines):
```
✅ Multi-Port UDP sender initialized (20 addresses)
Address allocation (20 total):
  Binance  → 8 addresses (4 trade + 4 orderbook)
  Others   → 2 addresses each (1 trade + 1 orderbook)

✅ All 20 multicast addresses initialized
```

### Receiver Startup (CRITICAL - Verify ALL 20 appear):
```
Starting Multi-Port UDP Receiver (20 addresses)
✅ Receiver binance_trade_0 bound to 0.0.0.0:9000 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_trade_0 successfully joined multicast group 239.10.1.1:9000
✅ Receiver binance_trade_1 bound to 0.0.0.0:9001 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_trade_1 successfully joined multicast group 239.10.1.1:9001
✅ Receiver binance_trade_2 bound to 0.0.0.0:9002 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_trade_2 successfully joined multicast group 239.10.1.1:9002
✅ Receiver binance_trade_3 bound to 0.0.0.0:9003 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_trade_3 successfully joined multicast group 239.10.1.1:9003
✅ Receiver binance_orderbook_0 bound to 0.0.0.0:9100 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_orderbook_0 successfully joined multicast group 239.10.1.1:9100
✅ Receiver binance_orderbook_1 bound to 0.0.0.0:9101 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_orderbook_1 successfully joined multicast group 239.10.1.1:9101
✅ Receiver binance_orderbook_2 bound to 0.0.0.0:9102 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_orderbook_2 successfully joined multicast group 239.10.1.1:9102
✅ Receiver binance_orderbook_3 bound to 0.0.0.0:9103 and joined multicast 239.10.1.1
DEBUG RECEIVER: binance_orderbook_3 successfully joined multicast group 239.10.1.1:9103
✅ Receiver bybit_trade_0 bound to 0.0.0.0:9000 and joined multicast 239.10.2.1
DEBUG RECEIVER: bybit_trade_0 successfully joined multicast group 239.10.2.1:9000
✅ Receiver bybit_orderbook_0 bound to 0.0.0.0:9100 and joined multicast 239.10.2.1
DEBUG RECEIVER: bybit_orderbook_0 successfully joined multicast group 239.10.2.1:9100
... (and so on for OKX, Deribit, Upbit, Coinbase, Bithumb)
✅ Started 20 receiver threads
```

### When Data Starts Flowing (FIRST PACKET LOGS):
```
DEBUG RECEIVER: binance_trade_0 received FIRST packet: 98 bytes from 192.168.1.10:54321
DEBUG RECEIVER: binance_orderbook_0 received FIRST packet: 130 bytes from 192.168.1.10:54322
DEBUG RECEIVER: bybit_trade_0 received FIRST packet: 98 bytes from 192.168.1.10:54323
...
```

### Ongoing Reception (Every 1000 packets):
```
DEBUG RECEIVER: binance_trade_0 received 1000 packets (0 MB total)
DEBUG RECEIVER: binance_trade_0 received 2000 packets (0 MB total)
DEBUG RECEIVER: binance_orderbook_0 received 1000 packets (0 MB total)
...
```

## Diagnostic Checklist

### ✅ Success Indicators:
1. **All 20 receivers show "successfully joined multicast group"** - Multicast joins working
2. **All receivers show "received FIRST packet"** - Data flowing correctly
3. **Periodic packet count logs appear** - Continuous data flow
4. **Receiver stats show packets > 0** - Data being processed

### ❌ Failure Scenarios:

#### Scenario 1: All Receivers Join But NO First Packets
**Symptoms:**
```
✅ Receiver binance_trade_0 ... successfully joined multicast group
... (all 20 show this)
✅ Started 20 receiver threads
(but no "received FIRST packet" logs appear)
```
**Diagnosis:** Network issue - packets not reaching receiver
**Solutions:**
1. Check firewall on receiver machine
2. Verify machines on same subnet (or multicast-capable router)
3. Check network switch supports IGMP and multicast forwarding
4. Run `netstat -g` to verify OS-level multicast group membership

#### Scenario 2: Some Receivers Get Data, Others Don't
**Symptoms:**
```
DEBUG RECEIVER: binance_trade_0 received FIRST packet ...
DEBUG RECEIVER: binance_trade_1 received FIRST packet ...
(but binance_trade_2, binance_trade_3 never show first packet)
```
**Diagnosis:** Partial multicast failure or port blocking
**Solutions:**
1. Check if specific ports are blocked (9002, 9003, 9102, 9103)
2. Verify all multicast addresses are being sent to by feeder
3. Check for buffer overflow on working receivers

#### Scenario 3: Packet Count Much Lower Than Sender
**Symptoms:**
```
Feeder: Sent 10000 packets
Receiver: Received 5000 packets total
```
**Diagnosis:** UDP packet loss (expected but high)
**Possible Causes:**
1. **Network congestion** - Too much traffic between machines
2. **Receiver buffer overflow** - Receiver can't keep up with sender rate
3. **Switch dropping multicast** - Switch not configured properly
4. **Late start** - Receiver started after sender had already sent packets

**Normal UDP Loss:** 1-5% packet loss is normal for UDP. 50%+ indicates serious issue.

## Verification Commands

### On Receiver Machine

#### Check if ports are bound:
```bash
# Windows
netstat -ano | findstr :9000
netstat -ano | findstr :9100

# Linux
netstat -tulpn | grep 9000
netstat -tulpn | grep 9100
```

#### Check multicast group membership:
```bash
# Windows (PowerShell as Admin)
netsh interface ipv4 show joins

# Linux
netstat -g
ip maddr show
```

#### Capture multicast packets with tcpdump:
```bash
# Linux
sudo tcpdump -i any -n 'dst net 239.10.0.0/16'

# Or specific address
sudo tcpdump -i any -n 'dst host 239.10.1.1'
```

### On Sender Machine

#### Verify sender is sending:
```bash
# Check feeder logs for:
"✅ All 20 multicast addresses initialized"
"Sent X packets" (in feeder output)
```

#### Capture outgoing multicast with tcpdump:
```bash
sudo tcpdump -i any -n 'dst net 239.10.0.0/16'
```

## Firewall Configuration

### Windows (Receiver Machine - Run as Admin):
```powershell
# Allow UDP ports 9000-9103
netsh advfirewall firewall add rule name="Multicast Receiver" dir=in action=allow protocol=UDP localport=9000-9103

# Allow multicast addresses 239.10.0.0/16
netsh advfirewall firewall add rule name="Multicast Group" dir=in action=allow protocol=UDP remoteip=239.10.0.0/255.255.0.0
```

### Linux (Receiver Machine):
```bash
# Allow UDP ports
sudo iptables -A INPUT -p udp --dport 9000:9103 -j ACCEPT

# Allow multicast addresses
sudo iptables -A INPUT -d 239.10.0.0/16 -j ACCEPT

# Save rules
sudo iptables-save > /etc/iptables/rules.v4
```

## Network Requirements

### Same Subnet (Simplest):
```
Sender:   192.168.1.10/24
Receiver: 192.168.1.20/24
✅ Should work with TTL=32
```

### Different Subnets (Requires Multicast Router):
```
Sender:   192.168.1.10/24
Receiver: 192.168.2.20/24
⚠️  Requires multicast-capable router with IGMP enabled
```

### Network Switch Requirements:
- Must support **IGMP (Internet Group Management Protocol)**
- Must have **multicast forwarding** enabled
- If unsure, test with direct ethernet cable between machines

## Troubleshooting Steps

### Step 1: Test on Same Machine
```bash
# Terminal 1
cargo run --release --bin feeder_direct

# Terminal 2
cargo run --release --bin multi_port_receiver
```

**If this works:** Network issue (firewall/switch/router)
**If this fails:** Code issue (check receiver joins multicast groups)

### Step 2: Test with Wireshark
On receiver machine, capture multicast traffic:
```
Display filter: ip.dst >= 239.10.1.1 and ip.dst <= 239.10.7.1
```

**Packets visible:** Firewall or application issue
**No packets:** Network or sender issue

### Step 3: Check OS Multicast Group Membership
```bash
# Windows
netsh interface ipv4 show joins

# Linux
ip maddr show
```

Should see entries like:
```
239.10.1.1
239.10.2.1
239.10.3.1
... (up to 239.10.7.1)
```

### Step 4: Verify Sender Addresses
Check feeder logs for confirmation it's sending to correct addresses:
```
Binance Trade pool: 239.10.1.1 ports 9000-9003
Binance OrderBook pool: 239.10.1.1 ports 9100-9103
Bybit Trade pool: 239.10.2.1 port 9000
Bybit OrderBook pool: 239.10.2.1 port 9100
...
```

## Performance Tuning

### If Packet Loss is High (>5%):

#### Increase Receiver Buffers:
Edit `multi_port_udp_receiver.rs:126`:
```rust
socket2.set_recv_buffer_size(8 * 1024 * 1024)?; // 8MB instead of 4MB
```

#### Increase Sender Buffers:
Edit `multi_port_udp_sender.rs:94`:
```rust
socket2.set_send_buffer_size(4 * 1024 * 1024)?; // 4MB instead of 2MB
```

#### OS-Level Buffer Tuning:
```bash
# Linux
sudo sysctl -w net.core.rmem_max=16777216
sudo sysctl -w net.core.wmem_max=16777216

# Windows (Registry - requires reboot)
# Set TcpWindowSize to higher value in:
# HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters
```

## Summary

**Key Files Modified:**
1. `multi_port_udp_receiver.rs` - Added debug output for multicast joins and packet reception
2. `multi_port_udp_sender.rs` - Fixed TTL to 32 for cross-machine capability
3. `binary_udp_sender.rs` - Fixed multicast configuration
4. `upbit.rs`, `deribit.rs`, `coinbase.rs`, `bithumb.rs` - Switched to multi_port_sender

**What to Watch:**
1. All 20 receivers successfully join multicast groups
2. All 20 receivers receive first packet within 30 seconds of feeder start
3. Packet counts increment steadily (every 1000 packets logged)
4. Packet loss < 5% is normal for UDP

**If Issues Persist:**
1. Test on same machine first
2. Check firewall on receiver
3. Verify network switch supports IGMP
4. Use Wireshark to see if packets reach receiver
5. Check OS-level multicast group membership with `netsh interface ipv4 show joins` or `ip maddr show`
