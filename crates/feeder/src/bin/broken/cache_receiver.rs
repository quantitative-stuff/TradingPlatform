/// UDP receiver that updates the symbol-centric lock-free cache
/// Receives binary packets and updates atomic slots

use feeder::core::{
    PacketHeader, OrderBookItem, TradeItem,
    MarketCache, SharedMarketCache,
};
use std::net::UdpSocket;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;

const CRYPTO_PORT: u16 = 9001;
const STOCK_PORT: u16 = 9002;
const MAX_PACKET_SIZE: usize = 1500;

/// Exchange name to ID mapping
fn get_exchange_id(name: &str) -> usize {
    match name.to_lowercase().as_str() {
        "binance" => 0,
        "bybit" => 1,
        "upbit" => 2,
        "coinbase" => 3,
        "okx" => 4,
        "deribit" => 5,
        "bithumb" => 6,
        "ls" => 7,
        _ => 0,
    }
}

/// Parse null-terminated string from fixed-size array
fn parse_name(bytes: &[u8]) -> String {
    let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..null_pos]).to_string()
}

/// UDP receiver thread
fn udp_receiver_thread(
    port: u16,
    cache: SharedMarketCache,
    symbol_map: Arc<parking_lot::RwLock<HashMap<String, usize>>>,
) {
    let addr = format!("0.0.0.0:{}", port);
    let socket = UdpSocket::bind(&addr).expect(&format!("Failed to bind to {}", addr));

    println!("[Receiver] Listening on {}", addr);

    let mut buf = [0u8; MAX_PACKET_SIZE];
    let mut packet_count = 0u64;
    let mut last_stats = SystemTime::now();

    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, _src)) => {
                if len < 58 {
                    eprintln!("[Receiver] Packet too small: {} bytes", len);
                    continue;
                }

                // Parse header
                let header: &PacketHeader = unsafe {
                    &*(buf.as_ptr() as *const PacketHeader)
                };

                // Validate protocol version
                if header.protocol_version != 1 {
                    eprintln!("[Receiver] Invalid protocol version: {}", header.protocol_version);
                    continue;
                }

                // Extract header fields
                let exchange_name = parse_name(&header.exchange);
                let symbol_name = parse_name(&header.symbol);
                let packet_type = (header.flags_and_count & 0b0111_0000) >> 4;
                let is_bid = (header.flags_and_count & 0b0000_1000) != 0;
                let item_count = (header.flags_and_count & 0b0000_0111) as usize;

                // Convert timestamps from network byte order
                let exchange_ts = u64::from_be(header.exchange_timestamp);
                let local_ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;

                // Map exchange and symbol to IDs
                let exchange_id = get_exchange_id(&exchange_name);

                // Get or create symbol ID
                let symbol_id = {
                    let read_map = symbol_map.read();
                    if let Some(&id) = read_map.get(&symbol_name) {
                        id
                    } else {
                        drop(read_map);
                        let mut write_map = symbol_map.write();
                        let id = write_map.len();
                        write_map.insert(symbol_name.clone(), id);
                        id
                    }
                };

                // Get cache slot
                if let Some(slot) = cache.get_slot(exchange_id, symbol_id) {
                    match packet_type {
                        1 => {
                            // Trade packet
                            if item_count > 0 && len >= 58 + 32 {
                                let trade_item: &TradeItem = unsafe {
                                    &*(buf[58..].as_ptr() as *const TradeItem)
                                };

                                let price = i64::from_be(trade_item.price);
                                slot.update_trade_fast(price, exchange_ts, local_ts);

                                packet_count += 1;
                            }
                        }
                        2 => {
                            // OrderBook packet
                            if item_count > 0 && len >= 58 + 16 {
                                let ob_item: &OrderBookItem = unsafe {
                                    &*(buf[58..].as_ptr() as *const OrderBookItem)
                                };

                                let price = i64::from_be(ob_item.price);
                                let qty = i64::from_be(ob_item.quantity);

                                if is_bid {
                                    slot.update_bid_fast(price, qty, exchange_ts, local_ts);
                                } else {
                                    slot.update_ask_fast(price, qty, exchange_ts, local_ts);
                                }

                                packet_count += 1;
                            }
                        }
                        _ => {
                            eprintln!("[Receiver] Unknown packet type: {}", packet_type);
                        }
                    }
                }

                // Print stats every 5 seconds
                if let Ok(elapsed) = SystemTime::now().duration_since(last_stats) {
                    if elapsed.as_secs() >= 5 {
                        println!(
                            "[Receiver] Port {} - Packets: {}, Rate: {}/s",
                            port,
                            packet_count,
                            packet_count / elapsed.as_secs()
                        );
                        packet_count = 0;
                        last_stats = SystemTime::now();
                    }
                }
            }
            Err(e) => {
                eprintln!("[Receiver] Error receiving packet: {}", e);
            }
        }
    }
}

fn main() {
    println!("=== Binary UDP Cache Receiver ===");
    println!("Protocol: Binary packets (58-byte header)");
    println!("Crypto port: {}, Stock port: {}", CRYPTO_PORT, STOCK_PORT);

    // Create cache
    // 8 exchanges (binance, bybit, upbit, coinbase, okx, deribit, bithumb, ls)
    // 1000 symbols per exchange (will grow dynamically via symbol_map)
    let cache = Arc::new(MarketCache::new(8, 1000));
    let symbol_map = Arc::new(parking_lot::RwLock::new(HashMap::new()));

    println!("Cache created: {} exchanges, {} symbol slots", 8, 1000);

    // Spawn receiver threads
    let cache_crypto = Arc::clone(&cache);
    let symbol_map_crypto = Arc::clone(&symbol_map);
    let crypto_thread = std::thread::spawn(move || {
        udp_receiver_thread(CRYPTO_PORT, cache_crypto, symbol_map_crypto);
    });

    let cache_stock = Arc::clone(&cache);
    let symbol_map_stock = Arc::clone(&symbol_map);
    let stock_thread = std::thread::spawn(move || {
        udp_receiver_thread(STOCK_PORT, cache_stock, symbol_map_stock);
    });

    println!("Receiver threads started. Press Ctrl+C to exit.");

    // Wait for threads
    crypto_thread.join().unwrap();
    stock_thread.join().unwrap();
}