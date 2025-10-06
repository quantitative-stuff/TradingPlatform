# Configuration Directory Structure

This directory contains all runtime configuration files for the feeder system.

## Directory Organization

```
config/
├── crypto/                      # Cryptocurrency exchange configurations
│   ├── binance_config_full.json
│   ├── bybit_config_full.json
│   ├── coinbase_config_full.json
│   ├── okx_config_full.json
│   ├── deribit_config_full.json
│   ├── bithumb_config_full.json
│   ├── upbit_config_full.json
│   └── symbol_mapping.json      # Symbol mapping across crypto exchanges
│
├── stock/                       # Stock exchange configurations
│   ├── k100_stock_full.json    # Full K100 stock list with futures
│   ├── k100_stock_sample.json  # Sample subset for testing
│   └── ls_config.json          # LS Exchange connection config
│
└── shared/                      # Shared configurations
    ├── feeder_config.json       # General feeder configuration
    └── crypto_exchanges.json    # List of active crypto exchanges
```

## Configuration Types

### Crypto Configs (`crypto/`)
- Exchange-specific WebSocket and REST API configurations
- Symbol lists for subscription
- Connection parameters (URLs, rate limits, retry settings)
- Market data types to subscribe (trades, orderbooks)
- `symbol_mapping.json`: Maps symbols across different crypto exchanges for unified handling

### Stock Configs (`stock/`)
- `k100_stock_full.json`: Complete list of Korean top 100 stocks with futures contracts
- `k100_stock_sample.json`: Smaller subset for development/testing
- `ls_config.json`: LS Exchange connection settings and credentials

### Shared Configs (`shared/`)
- `feeder_config.json`: System-wide settings (ports, logging, database connections)

## Usage

Configuration files are loaded by the Rust modules in `src/config/`:
- `src/config/load_config.rs` - Loads crypto exchange configs
- `src/config/load_config_ls.rs` - Loads stock exchange configs  
- `src/config/feeder_config.rs` - Loads general feeder config

## Environment-Specific Configs

For different environments (dev/staging/prod), you can:
1. Use environment variables to override config paths
2. Create environment-specific subdirectories (e.g., `config/dev/`, `config/prod/`)
3. Use the `LIMITED_MODE` environment variable for development (limits to 10 symbols)

## Security Notes

- Never commit API keys or secrets to these JSON files
- Use `src/config/secure_keys.rs` for loading sensitive data from environment variables
- Production configs should be managed separately from the repository