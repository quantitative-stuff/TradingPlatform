// UDP Packet Inspector - captures and analyzes binary packet structure
use tokio::net::UdpSocket;
use std::mem;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== UDP Packet Inspector ===");
    println!("Listening on 239.255.42.99:9001");
    println!("Will show raw bytes and parsed structure\n");

    // Bind to multicast address
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    socket.join_multicast_v4("239.255.42.99".parse()?, "0.0.0.0".parse()?)?;

    let mut buf = vec![0u8; 65536];
    let mut packet_count = 0;

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, addr)) => {
                packet_count += 1;
                println!("=== Packet #{} from {} ({} bytes) ===", packet_count, addr, len);

                if len >= 58 { // Minimum header size
                    inspect_packet(&buf[..len]);
                } else {
                    println!("⚠️  Packet too small for binary header: {} bytes", len);
                    hex_dump(&buf[..len], 0);
                }

                println!();

                if packet_count >= 10 {
                    println!("Stopping after 10 packets. Restart to see more.");
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error receiving packet: {}", e);
            }
        }
    }

    Ok(())
}

fn inspect_packet(data: &[u8]) {
    println!("Raw packet hex dump:");
    hex_dump(data, 0);

    if data.len() >= 58 {
        println!("\n📋 Header Analysis (58 bytes):");

        // Parse header fields manually
        let protocol_version = data[0];
        let exchange_timestamp = u64::from_be_bytes([
            data[1], data[2], data[3], data[4],
            data[5], data[6], data[7], data[8]
        ]);
        let local_timestamp = u64::from_be_bytes([
            data[9], data[10], data[11], data[12],
            data[13], data[14], data[15], data[16]
        ]);
        let flags_and_count = data[17];

        // Extract symbol (20 bytes, null-terminated)
        let symbol_bytes = &data[18..38];
        let symbol = String::from_utf8_lossy(symbol_bytes).trim_end_matches('\0');

        // Extract exchange (20 bytes, null-terminated)
        let exchange_bytes = &data[38..58];
        let exchange = String::from_utf8_lossy(exchange_bytes).trim_end_matches('\0');

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

        // Parse payload based on type
        if data.len() > 58 {
            println!("\n📦 Payload Analysis ({} bytes):", data.len() - 58);
            let payload = &data[58..];

            match packet_type {
                1 => parse_trade_payload(payload),
                2 => parse_orderbook_payload(payload, is_bid, count),
                _ => {
                    println!("  Unknown packet type, showing raw payload:");
                    hex_dump(payload, 58);
                }
            }
        }
    }
}

fn parse_trade_payload(payload: &[u8]) {
    println!("  Trade Data:");
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

        println!("    Trade ID: {}", trade_id);
        println!("    Price: {} (scaled: {})", price, price_scaled);
        println!("    Quantity: {} (scaled: {})", quantity, quantity_scaled);
        println!("    Is Buyer Maker: {}", is_buyer_maker);

        if payload.len() > 32 {
            println!("    Extra payload bytes:");
            hex_dump(&payload[32..], 90);
        }
    } else {
        println!("    ⚠️  Trade payload too small: {} bytes", payload.len());
        hex_dump(payload, 58);
    }
}

fn parse_orderbook_payload(payload: &[u8], is_bid: bool, count: u8) {
    println!("  OrderBook Data ({}):", if is_bid { "Bids" } else { "Asks" });
    println!("    Expected items: {}", count);

    let item_size = 16; // Each orderbook item is 16 bytes
    let expected_size = count as usize * item_size;

    if payload.len() >= expected_size {
        for i in 0..count {
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

            println!("    Level {}: Price={} Qty={} (scaled: {}, {})",
                     i + 1, price, quantity, price_scaled, quantity_scaled);
        }

        if payload.len() > expected_size {
            println!("    Extra payload bytes:");
            hex_dump(&payload[expected_size..], 58 + expected_size);
        }
    } else {
        println!("    ⚠️  OrderBook payload too small: {} bytes, expected {}",
                payload.len(), expected_size);
        hex_dump(payload, 58);
    }
}

fn hex_dump(data: &[u8], offset: usize) {
    const BYTES_PER_LINE: usize = 16;

    for (i, chunk) in data.chunks(BYTES_PER_LINE).enumerate() {
        let addr = offset + i * BYTES_PER_LINE;
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