/// Multi-port UDP receiver
/// Spawns one thread per port for parallel processing

use feeder::core::{
    PacketHeader, OrderBookItem, TradeItem,
    MarketCache, SharedMarketCache,
};
use std::net::UdpSocket;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;

const BASE_PORT: u16 = 9001;
const NUM_PORTS: usize = 4;
const MAX_PACKET_SIZE: usize = 1500;

/// Get exchange ID from name
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

/// Parse null-terminated string
fn parse_name(bytes: &[u8]) -> String {
    let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..null_pos]).to_string()
}

/// UDP receiver thread for one port
fn udp_receiver_thread(
    port: u16,
    cache: SharedMarketCache,
    symbol_map: Arc<parking_lot::RwLock<HashMap<String, usize>>>,
) {
    let addr = format!("0.0.0.0:{}", port);
    let socket = UdpSocket::bind(&addr).expect(&format!("Failed to bind to {}", addr));

    println!("[Port {}] Listening on {}", port, addr);

    let mut buf = [0u8; MAX_PACKET_SIZE];
    let mut packet_count = 0u64;
    let mut last_stats = SystemTime::now();

    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, _src)) => {
                if len < 58 {
                    continue;
                }

                // Parse header
                let header: &PacketHeader = unsafe {
                    &*(buf.as_ptr() as *const PacketHeader)
                };

                if header.protocol_version != 1 {
                    continue;
                }

                // Extract fields
                let exchange_name = parse_name(&header.exchange);
                let symbol_name = parse_name(&header.symbol);
                let packet_type = (header.flags_and_count & 0b0111_0000) >> 4;
                let is_bid = (header.flags_and_count & 0b0000_1000) != 0;
                let item_count = (header.flags_and_count & 0b0000_0111) as usize;

                let exchange_ts = u64::from_be(header.exchange_timestamp);
                let local_ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64;

                // Map to IDs
                let exchange_id = get_exchange_id(&exchange_name);

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

                // Update cache
                if let Some(slot) = cache.get_slot(exchange_id, symbol_id) {
                    match packet_type {
                        1 => {
                            // Trade
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
                            // OrderBook
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
                        _ => {}
                    }
                }

                // Stats every 5 seconds
                if let Ok(elapsed) = SystemTime::now().duration_since(last_stats) {
                    if elapsed.as_secs() >= 5 {
                        println!(
                            "[Port {}] Packets: {}, Rate: {}/s",
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
                eprintln!("[Port {}] Error: {}", port, e);
            }
        }
    }
}

fn main() {
    println!("=== Multi-Port UDP Receiver ===");
    println!("Listening on ports {} - {}", BASE_PORT, BASE_PORT + NUM_PORTS as u16 - 1);
    println!();

    // Create shared cache
    let cache = Arc::new(MarketCache::new(8, 1000));
    let symbol_map = Arc::new(parking_lot::RwLock::new(HashMap::new()));

    println!("Cache created: 8 exchanges, 1000 symbol slots");
    println!();

    // Spawn receiver thread for each port
    let mut handles = vec![];

    for i in 0..NUM_PORTS {
        let port = BASE_PORT + i as u16;
        let cache_clone = Arc::clone(&cache);
        let symbol_map_clone = Arc::clone(&symbol_map);

        let handle = std::thread::spawn(move || {
            udp_receiver_thread(port, cache_clone, symbol_map_clone);
        });

        handles.push(handle);
    }

    println!("Started {} receiver threads", NUM_PORTS);
    println!("Press Ctrl+C to exit");
    println!();

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}