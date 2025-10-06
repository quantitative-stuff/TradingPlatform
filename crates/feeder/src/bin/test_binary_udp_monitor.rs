use tokio::net::UdpSocket;
use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 Binary UDP Packet Size Monitor");
    println!("📡 Listening on: 239.255.42.99:9001");

    // Bind to multicast address
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;

    // Join multicast group
    socket.join_multicast_v4("239.255.42.99".parse()?, "0.0.0.0".parse()?)?;

    let mut buffer = [0u8; 65536];
    let mut packet_count = 0;
    let mut total_bytes = 0;
    let mut binary_packets = 0;
    let mut text_packets = 0;
    let mut trade_packets = 0;
    let mut orderbook_packets = 0;

    let start_time = Instant::now();

    println!("⏰ Starting packet monitoring...");

    loop {
        match socket.recv_from(&mut buffer).await {
            Ok((len, _addr)) => {
                packet_count += 1;
                total_bytes += len;

                // Check if it's a binary packet (starts with protocol version 1)
                if len >= 66 && buffer[0] == 1 {
                    binary_packets += 1;

                    // Check packet type from header
                    let flags_and_count = buffer[25];
                    let packet_type = (flags_and_count & 0b0100_0000) >> 6;
                    let item_count = flags_and_count & 0b0001_1111;

                    if packet_type == 0 {
                        orderbook_packets += 1;
                        if orderbook_packets <= 5 {
                            println!("📊 Binary OrderBook packet: {} bytes ({} items)", len, item_count);
                        }
                    } else if packet_type == 1 {
                        trade_packets += 1;
                        if trade_packets <= 5 {
                            println!("💰 Binary Trade packet: {} bytes", len);
                        }
                    }
                } else {
                    // Text packet
                    text_packets += 1;
                    let text = String::from_utf8_lossy(&buffer[..len]);

                    if text_packets <= 3 {
                        if text.starts_with("STATS") {
                            println!("📈 Stats packet: {} bytes - {}", len, text);
                        } else if text.starts_with("CONN") {
                            println!("🔗 Connection packet: {} bytes - {}", len, text);
                        } else {
                            println!("📄 Text packet: {} bytes - {}", len, text.chars().take(50).collect::<String>());
                        }
                    }
                }

                // Print statistics every 10 seconds
                if start_time.elapsed().as_secs() >= 10 {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let packets_per_sec = packet_count as f64 / elapsed;
                    let bytes_per_sec = total_bytes as f64 / elapsed;
                    let mbps = (bytes_per_sec * 8.0) / 1_000_000.0;

                    println!("\n========================================");
                    println!("📊 BINARY UDP MONITORING RESULTS:");
                    println!("⏰ Elapsed: {:.1} seconds", elapsed);
                    println!("📦 Total packets: {}", packet_count);
                    println!("📈 Packets/sec: {:.1}", packets_per_sec);
                    println!("📊 Total bytes: {}", total_bytes);
                    println!("🚀 Bytes/sec: {:.0}", bytes_per_sec);
                    println!("📡 Bandwidth: {:.2} Mbps", mbps);
                    println!();
                    println!("🔍 PACKET BREAKDOWN:");
                    println!("📊 Binary packets: {} ({:.1}%)", binary_packets,
                        binary_packets as f64 / packet_count as f64 * 100.0);
                    println!("   - Trade packets: {}", trade_packets);
                    println!("   - OrderBook packets: {}", orderbook_packets);
                    println!("📄 Text packets: {} ({:.1}%)", text_packets,
                        text_packets as f64 / packet_count as f64 * 100.0);

                    if binary_packets > 0 {
                        let avg_binary_size = (total_bytes - (text_packets * 50)) / binary_packets; // Rough estimate
                        println!("📏 Average binary packet size: ~{} bytes", avg_binary_size);

                        if mbps < 10.0 {
                            println!("✅ SUCCESS! Bandwidth significantly reduced!");
                            println!("   Binary format working correctly.");
                        } else {
                            println!("⚠️  Bandwidth still high - check if binary format is being used");
                        }
                    }

                    println!("========================================\n");
                    break;
                }
            }
            Err(e) => {
                eprintln!("❌ Error receiving packet: {}", e);
                break;
            }
        }
    }

    Ok(())
}