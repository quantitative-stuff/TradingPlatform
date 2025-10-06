// Test Binary UDP Packet Creation and Inspection
use feeder::core::{TradeData, OrderBookData, BinaryUdpPacket};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("=== Binary UDP Packet Inspection Test ===\n");

    // Create sample trade data
    let trade = TradeData {
        exchange: "binance".to_string(),
        symbol: "BTCUSDT".to_string(),
        asset_type: "spot".to_string(),
        price: 45000.50,
        quantity: 1.25,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    };

    println!("📊 Creating binary packet from trade data:");
    println!("  Exchange: {}", trade.exchange);
    println!("  Symbol: {}", trade.symbol);
    println!("  Price: {}", trade.price);
    println!("  Quantity: {}", trade.quantity);
    println!("  Timestamp: {} ms", trade.timestamp);

    // Create binary packet
    let mut binary_packet = BinaryUdpPacket::from_trade(&trade);
    let packet_bytes = binary_packet.to_bytes();

    println!("\n🔍 Binary packet analysis:");
    println!("  Total packet size: {} bytes", packet_bytes.len());
    println!("  Header size: 58 bytes");
    println!("  Payload size: {} bytes", packet_bytes.len() - 58);

    // Show raw hex dump
    println!("\n📋 Raw packet hex dump:");
    hex_dump(&packet_bytes);

    // Parse header manually
    println!("\n🔬 Manual header parsing:");
    if packet_bytes.len() >= 58 {
        parse_header(&packet_bytes[..58]);
    }

    // Parse payload
    if packet_bytes.len() > 58 {
        println!("\n📦 Payload analysis:");
        parse_trade_payload(&packet_bytes[58..]);
    }

    // Test with OrderBook data
    println!("\n{}", "=".repeat(60));
    println!("📈 Testing OrderBook packet:");

    let orderbook = OrderBookData {
        exchange: "okx".to_string(),
        symbol: "ETHUSDT".to_string(),
        asset_type: "spot".to_string(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        bids: vec![
            (3000.10, 5.0),
            (3000.05, 3.2),
        ],
        asks: vec![
            (3000.15, 2.8),
            (3000.20, 4.1),
        ],
    };

    // Test bids packet
    let mut bids_packet = BinaryUdpPacket::from_orderbook(&orderbook, true);
    let bids_bytes = bids_packet.to_bytes();

    println!("  Bids packet size: {} bytes", bids_bytes.len());
    println!("  Expected: 58 (header) + 32 (2 levels × 16 bytes) = 90 bytes");

    if bids_bytes.len() >= 58 {
        println!("\n🔬 Bids packet header:");
        parse_header(&bids_bytes[..58]);
    }

    if bids_bytes.len() > 58 {
        println!("\n📦 Bids payload:");
        parse_orderbook_payload(&bids_bytes[58..], true, 2);
    }
}

fn hex_dump(data: &[u8]) {
    const BYTES_PER_LINE: usize = 16;

    for (i, chunk) in data.chunks(BYTES_PER_LINE).enumerate() {
        let addr = i * BYTES_PER_LINE;
        print!("  {:04X}: ", addr);

        // Print hex bytes
        for (j, byte) in chunk.iter().enumerate() {
            print!("{:02X} ", byte);
            if j == 7 { print!(" "); } // Extra space in middle
        }

        // Pad if line is short
        for _ in chunk.len()..BYTES_PER_LINE {
            print!("   ");
        }
        if chunk.len() <= 8 { print!(" "); }

        // Print ASCII representation
        print!(" |");
        for byte in chunk {
            let c = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            };
            print!("{}", c);
        }
        println!("|");
    }
}

fn parse_header(header_bytes: &[u8]) {
    let protocol_version = header_bytes[0];
    let exchange_timestamp = u64::from_be_bytes([
        header_bytes[1], header_bytes[2], header_bytes[3], header_bytes[4],
        header_bytes[5], header_bytes[6], header_bytes[7], header_bytes[8]
    ]);
    let local_timestamp = u64::from_be_bytes([
        header_bytes[9], header_bytes[10], header_bytes[11], header_bytes[12],
        header_bytes[13], header_bytes[14], header_bytes[15], header_bytes[16]
    ]);
    let flags_and_count = header_bytes[17];

    // Extract symbol (20 bytes, null-terminated)
    let symbol_bytes = &header_bytes[18..38];
    let symbol_string = String::from_utf8_lossy(symbol_bytes);
    let symbol = symbol_string.trim_end_matches('\0');

    // Extract exchange (20 bytes, null-terminated)
    let exchange_bytes = &header_bytes[38..58];
    let exchange_string = String::from_utf8_lossy(exchange_bytes);
    let exchange = exchange_string.trim_end_matches('\0');

    println!("  Protocol Version: {} (0x{:02X})", protocol_version, protocol_version);
    println!("  Exchange Timestamp: {} ns", exchange_timestamp);
    println!("  Local Timestamp: {} ns", local_timestamp);
    println!("  Flags & Count: {} (0b{:08b})", flags_and_count, flags_and_count);
    println!("  Symbol: '{}'", symbol);
    println!("  Exchange: '{}'", exchange);

    // Decode flags
    let is_last = (flags_and_count & 0x80) != 0;
    let packet_type = (flags_and_count >> 4) & 0x07;
    let is_bid = (flags_and_count & 0x08) != 0;
    let count = flags_and_count & 0x07;

    println!("  Decoded flags:");
    println!("    is_last: {}", is_last);
    println!("    packet_type: {} ({})", packet_type, match packet_type {
        1 => "Trade",
        2 => "OrderBook",
        _ => "Unknown"
    });
    println!("    is_bid: {}", is_bid);
    println!("    count: {}", count);
}

fn parse_trade_payload(payload: &[u8]) {
    if payload.len() >= 32 {
        let trade_id = u64::from_be_bytes([
            payload[0], payload[1], payload[2], payload[3],
            payload[4], payload[5], payload[6], payload[7]
        ]);
        let price_scaled = i64::from_be_bytes([
            payload[8], payload[9], payload[10], payload[11],
            payload[12], payload[13], payload[14], payload[15]
        ]);
        let quantity_scaled = i64::from_be_bytes([
            payload[16], payload[17], payload[18], payload[19],
            payload[20], payload[21], payload[22], payload[23]
        ]);
        let is_buyer_maker = payload[24] != 0;

        let price = price_scaled as f64 / 100_000_000.0;
        let quantity = quantity_scaled as f64 / 100_000_000.0;

        println!("  Trade ID: {}", trade_id);
        println!("  Price: {} (scaled: {})", price, price_scaled);
        println!("  Quantity: {} (scaled: {})", quantity, quantity_scaled);
        println!("  Is Buyer Maker: {}", is_buyer_maker);

        // Verify scaling
        println!("  Scaling verification:");
        println!("    Original price: 45000.50");
        println!("    Scaled: {} (45000.50 × 10^8 = {})", price_scaled, (45000.50 * 100_000_000.0) as i64);
        println!("    Recovered: {}", price);
    } else {
        println!("  ⚠️  Trade payload too small: {} bytes", payload.len());
    }
}

fn parse_orderbook_payload(payload: &[u8], is_bid: bool, expected_count: u8) {
    println!("  Type: {}", if is_bid { "Bids" } else { "Asks" });
    println!("  Expected items: {}", expected_count);

    let item_size = 16; // Each orderbook item is 16 bytes
    let expected_size = expected_count as usize * item_size;

    if payload.len() >= expected_size {
        for i in 0..expected_count {
            let offset = i as usize * item_size;
            let item_data = &payload[offset..offset + item_size];

            let price_scaled = i64::from_be_bytes([
                item_data[0], item_data[1], item_data[2], item_data[3],
                item_data[4], item_data[5], item_data[6], item_data[7]
            ]);
            let quantity_scaled = i64::from_be_bytes([
                item_data[8], item_data[9], item_data[10], item_data[11],
                item_data[12], item_data[13], item_data[14], item_data[15]
            ]);

            let price = price_scaled as f64 / 100_000_000.0;
            let quantity = quantity_scaled as f64 / 100_000_000.0;

            println!("  Level {}: Price={} Qty={} (raw: {}, {})",
                     i + 1, price, quantity, price_scaled, quantity_scaled);
        }
    } else {
        println!("  ⚠️  Payload too small: {} bytes, expected {}", payload.len(), expected_size);
    }
}