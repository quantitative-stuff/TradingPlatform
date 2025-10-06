# TradingPlatform

High-frequency trading platform for cryptocurrency and stock markets.

## Architecture

This monorepo contains all components for real-time market data ingestion, feature engineering, signal generation, and trading execution.

### Components

- **market-types**: Shared data structures (exchanges, trades, order books)
- **udp-protocol**: Unified UDP multicast protocol (sender/receiver)
- **feeder**: Multi-exchange data ingestion (WebSocket → UDP)
- **matching-engine**: L2 to L3 order book reconstruction
- **feature-engineering**: Real-time feature computation (WAP, OFI, imbalances)
- **signal-generation**: Trading signal generation
- **backtesting**: Event-driven backtesting engine
- **oms**: Order Management System with risk controls
- **database**: QuestDB/KDB+ adapters for persistence
- **historical-loader**: Historical data readers (TARDIS, Binance)

## Getting Started

### Prerequisites

- Rust 1.75+
- QuestDB (optional, for logging)

### Build

```bash
# Build all crates
cargo build --release

# Build specific crate
cargo build -p feeder --release

# Run tests
cargo test
```

### Run

```bash
# Start data feeder
cargo run -p feeder

# Start live trading platform
cargo run -p live-trading

# Run backtest
cargo run -p backtest
```

## Project Structure

```
TradingPlatform/
├── crates/          # Library crates
├── bins/            # Binary applications
├── tests/           # Integration tests
├── docs/            # Documentation
└── config/          # Configuration files
```

## Development

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for development guidelines.

## License

Proprietary
