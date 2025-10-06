use feeder::crypto::feed::OkxExchange;
use feeder::core::{Feeder, init_global_binary_udp_sender, SymbolMapper};
use feeder::load_config::load_exchange_config;
use std::sync::Arc;
use std::net::UdpSocket;
use tokio;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Testing OKX Complete Data Flow (WebSocket → UDP) ===\n");

    // Initialize UDP sender
    println!("1. Initializing UDP sender...");
    init_global_binary_udp_sender("0.0.0.0:0", "239.255.42.99:9001").await?;
    println!("   ✅ UDP sender initialized\n");

    // Load OKX config
    println!("2. Loading OKX config...");
    let config = load_exchange_config("config/crypto/okx_config.json")?;
    println!("   ✅ Config loaded");
    println!("   Spot symbols: {:?}", config.subscribe_data.spot_symbols);
    println!("   Futures symbols: {:?}\n", config.subscribe_data.futures_symbols);

    // Create UDP listener to monitor packets
    println!("3. Setting up UDP listener on port 9001...");
    let udp_socket = UdpSocket::bind("127.0.0.1:9001")?;
    udp_socket.set_nonblocking(true)?;
    println!("   ✅ UDP listener ready\n");

    // Create and start OKX exchange
    println!("4. Starting OKX exchange...");
    let symbol_mapper = Arc::new(SymbolMapper::new());
    let okx = OkxExchange::new(config, symbol_mapper);

    // Start OKX in background
    tokio::spawn(async move {
        okx.start("spot").await;
    });

    println!("   ✅ OKX started\n");

    // Monitor UDP packets
    println!("5. Monitoring UDP packets for 20 seconds...\n");
    let mut buffer = [0u8; 65536];
    let mut packet_count = 0;
    let mut trade_count = 0;
    let mut orderbook_count = 0;
    let start = std::time::Instant::now();

    while start.elapsed() < std::time::Duration::from_secs(20) {
        match udp_socket.recv_from(&mut buffer) {
            Ok((size, _addr)) => {
                packet_count += 1;

                // Check packet type (simplified check)
                if size > 66 {
                    // Binary packet header is 66 bytes
                    // Check if it's trade or orderbook based on packet content
                    let packet_type = if buffer[0..10].windows(5).any(|w| w == b"trade") {
                        trade_count += 1;
                        "TRADE"
                    } else if buffer[0..10].windows(4).any(|w| w == b"book") {
                        orderbook_count += 1;
                        "ORDERBOOK"
                    } else {
                        "UNKNOWN"
                    };

                    if packet_count == 1 || packet_count % 10 == 0 {
                        println!("📦 Packet #{}: {} (size: {} bytes)", packet_count, packet_type, size);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, continue
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
            Err(e) => {
                println!("❌ UDP receive error: {}", e);
                break;
            }
        }
    }

    println!("\n=== Test Results ===");
    println!("📊 Total UDP packets received: {}", packet_count);
    println!("💰 Trade packets: {}", trade_count);
    println!("📚 OrderBook packets: {}", orderbook_count);

    if packet_count > 0 {
        println!("\n✅ SUCCESS: OKX is feeding data through UDP!");
    } else {
        println!("\n❌ FAILED: No UDP packets received!");
        println!("   OKX WebSocket may be working but UDP distribution is not.");
    }

    Ok(())
}