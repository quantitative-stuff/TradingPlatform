# HFT Migration Pattern: f64 → Scaled i64

## OLD WAY (Using f64)
```rust
let trade = TradeData {
    price: actual_data["p"].as_str()
        .and_then(|s| s.parse::<f64>().ok())  // ❌ f64!
        .unwrap_or_default(),
    quantity: actual_data["q"].as_str()
        .and_then(|s| s.parse::<f64>().ok())  // ❌ f64!
        .unwrap_or_default(),
    //...
};
```

## NEW WAY (Direct to scaled i64 - TRUE HFT!)
```rust
use crate::core::parse_to_scaled_or_default;

// Get precision from config (or use defaults)
let price_precision = config.feed_config.get_price_precision(&symbol);
let qty_precision = config.feed_config.get_quantity_precision(&symbol);

let trade = TradeData {
    price: actual_data["p"].as_str()
        .map(|s| parse_to_scaled_or_default(s, price_precision))  // ✅ NO f64!
        .unwrap_or(0),
    quantity: actual_data["q"].as_str()
        .map(|s| parse_to_scaled_or_default(s, qty_precision))    // ✅ NO f64!
        .unwrap_or(0),
    price_precision,
    quantity_precision,
    timestamp: /* ... */,
    timestamp_unit: crate::load_config::TimestampUnit::Milliseconds,
};
```

## For OrderBooks
```rust
// OLD: Vec<(f64, f64)>
let bids_f64: Vec<(f64, f64)> = /* parse bids */;

// NEW: Parse directly to Vec<(i64, i64)>
let bids: Vec<(i64, i64)> = actual_data["bids"]
    .as_array()
    .unwrap_or(&vec![])
    .iter()
    .filter_map(|bid| {
        let bid_arr = bid.as_array()?;
        let price_str = bid_arr[0].as_str()?;
        let qty_str = bid_arr[1].as_str()?;

        // ✅ Direct string → scaled i64
        let price = parse_to_scaled(price_str, price_precision)?;
        let qty = parse_to_scaled(qty_str, qty_precision)?;

        Some((price, qty))
    })
    .collect();
```

## Apply to All 7 Exchanges
1. Binance
2. Bybit
3. Coinbase
4. Upbit
5. Bithumb
6. Deribit
7. OKX

Each needs the same pattern:
- Import `parse_to_scaled_or_default`
- Get precision from config
- Parse strings directly to i64
- Add precision fields to structs
