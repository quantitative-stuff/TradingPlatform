# Trading Platform - Project Context

## Project Overview
High-performance, multi-language trading platform combining Rust feeders with Julia analytics for cryptocurrency and stock market trading. The system processes real-time market data through WebSocket connections, distributes via UDP multicast, and supports both operational monitoring and quantitative analysis.

## Architecture

### System Components
```
1. Data Collection (Rust)
   ├── Crypto Exchanges: Binance, Bybit, Upbit, Coinbase, OKX, Deribit, Bithumb
   ├── Stock Exchange: LS Exchange
   └── WebSocket + REST API connections

2. Data Distribution (UDP Multicast)
   ├── Port 9001: Crypto data
   ├── Port 9002: Stock data
   └── Protocol: Text format for monitoring, Binary for low-latency

3. Data Processing (Julia/Rust)
   ├── Matching Engine: L2→L3 conversion
   ├── Feature Engineering: WAP, OFI, TFI, spreads
   └── Signal Generation: Predictive models

4. Trading Execution
   ├── Order Management System (OMS)
   ├── Risk Management
   └── Backtesting Framework

5. Storage & Monitoring
   ├── QuestDB: Operational logs, connection status
   ├── KDB+: High-frequency tick data (planned)
   └── Real-time TUI monitors
```

## Codebase Structure

### Workspace Organization (Rust)
```
crates/
├── feeder/               # Exchange connections & data feed
│   ├── crypto/feed/      # Exchange-specific WebSocket handlers
│   ├── crypto/rest/      # REST API implementations
│   ├── stock/            # Stock exchange adapters
│   └── bin/              # Executable entry points
├── market-types/         # Shared data structures
├── udp-protocol/         # UDP multicast sender/receiver
├── matching-engine/      # Order book reconstruction, L2→L3
├── feature-engineering/  # Online feature computation
├── signal-generation/    # Trading signal logic
├── backtesting/          # Strategy backtesting engine
├── oms/                  # Order management system
├── database/             # Database adapters (QuestDB, KDB+)
└── historical-loader/    # Historical data processing
```

### Configuration
```
config/
├── feeder/
│   ├── crypto/           # Exchange-specific configs
│   └── stock/            # Stock exchange configs
```

## Data Flow Pipeline

### 1. Real-time Data Collection
```
Exchange WebSocket → Parse → Normalize → Global State → UDP Broadcast
```
- **Connection Management**: 5 symbols per WebSocket (rate limit optimization)
- **Reconnection**: Exponential backoff with exchange-specific delays
- **State Storage**: Thread-safe circular buffers (10k capacity)

### 2. UDP Distribution Protocol
- **Text Format**: `EXCHANGE|name|type|counts|timestamp` (monitoring/debugging)
- **Binary Format**: 66-byte header + payload (low-latency trading)
- **Connection Status**: `CONN|exchange|connected|disconnected|total`

### 3. Feature Generation Pipeline
```
UDP Packets → Matching Engine → Features → Signals → Orders
```
- **NBBO**: Cross-exchange best bid/offer
- **WAP**: 200ms lagged weighted average price
- **Online Features**: OFI, TFI, spread, order book imbalance
- **Target**: 15-second forward price prediction

### 4. Signal & Execution
- **Signal Generation**: Feature aggregation with predictive models
- **Risk Management**: Position limits, correlation-based portfolio risk
- **Order Routing**: Smart routing across exchanges
- **Backtesting**: Vectorized with realistic 200ms latency assumption

## Key Technical Details

### Exchange Integration
- **Crypto Exchanges**:
  - Binance, Bybit, Upbit (high liquidity)
  - Coinbase, OKX, Deribit, Bithumb (regional/specialized)
- **Connection Limits**: Exchange-specific rate limits and symbol caps
- **Data Types**: Trades, order book snapshots, incremental updates

### Performance Optimization
- **Language**: Rust for low-latency data feed, Julia for numerical computation
- **Async Runtime**: Tokio for concurrent WebSocket management
- **Memory**: Circular buffers, zero-copy where possible
- **Serialization**: SIMD-JSON for parsing, bincode for binary protocol

### Data Quality & Validation
- **Timestamp Checks**: Monotonic increases, clock drift detection
- **Price Validation**: Outlier detection, crossed market checks
- **Volume Validation**: Abnormal spike detection
- **Gap Detection**: Sequence number tracking, recovery mechanisms

### Operational Environment
- **Development**: `LIMITED_MODE=true` (10 symbols per category)
- **Production**: File logging only, systemd service deployment
- **Monitoring**: Real-time TUI (websocket_monitor, udp_monitor)
- **Scaling**: Multiple consumers via UDP multicast

## Development Workflow

### Running the Feeder
```bash
# Development mode (limited symbols)
LIMITED_MODE=true cargo run --bin feeder_direct

# Production mode
cargo build --release
./target/release/feeder_direct
```

### Monitoring
```bash
# WebSocket connections
cargo run --bin websocket_monitor

# UDP packet flow
cargo run --bin udp_monitor
```

### Testing Pipeline
1. Unit tests per crate
2. Integration tests with mock exchanges
3. Forward testing with historical data
4. Paper trading validation

## Project Goals & Constraints

### Primary Objectives
- Sub-millisecond feed latency
- 99.99% uptime with automatic recovery
- Support for 1000+ symbols across exchanges
- Real-time feature computation for HFT signals

### Technical Constraints
- Exchange rate limits (5 symbols/WebSocket typical)
- Network latency (200ms assumed for backtesting)
- Memory limits (10k buffer per symbol)
- UDP packet size (MTU considerations)

## Future Enhancements
- KDB+ integration for tick data storage
- Additional exchange support
- Hardware acceleration for feature computation
- Co-location deployment strategies
- Machine learning model integration

## Important Notes
- Production uses file logging only (no terminal output)
- Each exchange has unique parameters and limitations
- UDP multicast allows multiple consumer processes
- Database writes are separated from feed logic for reliability
- no simplifed versions. only consider full implementation
- never use synthetic data or fake data or man-made data ALWAYS use real data
- do not try to go shortcut always stay in the core logic even if it gets complicated and takes more time
- never use any shortcut or simplified version
