use std::net::UdpSocket;
use std::time::{Duration, Instant};

fn main() -> std::io::Result<()> {
    println!("🔍 Simple UDP Listener on 127.0.0.1:9002 (avoiding conflict with 9001)");

    // Try multicast address first
    let socket = UdpSocket::bind("0.0.0.0:9002")?;
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;

    // Join multicast group
    socket.join_multicast_v4(
        &"239.255.42.99".parse().unwrap(),
        &"0.0.0.0".parse().unwrap()
    )?;

    println!("✅ Joined multicast group 239.255.42.99:9001");
    println!("📡 Listening for UDP packets...");

    let mut buffer = [0u8; 65535];
    let mut packet_count = 0;
    let mut total_bytes = 0;
    let start_time = Instant::now();
    let duration = Duration::from_secs(30);

    while start_time.elapsed() < duration {
        match socket.recv_from(&mut buffer) {
            Ok((size, addr)) => {
                packet_count += 1;
                total_bytes += size;

                // Show first few packets in detail
                if packet_count <= 5 {
                    println!("📦 Packet #{}: {} bytes from {}", packet_count, size, addr);

                    // Try to parse as text first (connection status messages)
                    if let Ok(text) = std::str::from_utf8(&buffer[..size]) {
                        if text.contains("CONN") || text.contains("|") {
                            println!("   Text: {}", text.trim());
                        }
                    } else if size >= 66 {
                        // Binary packet - show basic info
                        let protocol_version = buffer[0];
                        println!("   Binary packet - Protocol v{}", protocol_version);
                    }
                }

                // Progress update every 10 packets
                if packet_count % 10 == 0 {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let rate = packet_count as f64 / elapsed;
                    println!("📊 Progress: {} packets, {} bytes total, {:.1} packets/s",
                        packet_count, total_bytes, rate);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout - continue
            },
            Err(e) => {
                eprintln!("Error receiving packet: {}", e);
                break;
            }
        }
    }

    println!("\n========================================");
    println!("📊 RESULTS ({:.1}s):", start_time.elapsed().as_secs_f64());
    println!("   Total packets: {}", packet_count);
    println!("   Total bytes: {}", total_bytes);

    if packet_count == 0 {
        println!("⚠️  No UDP packets received!");
        println!("   - Check if feeder_direct is running");
        println!("   - Check multicast configuration");
    } else {
        let rate = packet_count as f64 / start_time.elapsed().as_secs_f64();
        println!("   Average rate: {:.1} packets/s", rate);
        println!("✅ UDP data is flowing!");
    }

    Ok(())
}