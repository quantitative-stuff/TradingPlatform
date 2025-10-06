use tokio::net::UdpSocket;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔍 UDP Test Receiver");
    println!("📡 Listening on: 0.0.0.0:9001");
    println!("📡 Multicast group: 239.255.42.99");

    // Bind to port 9001
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    println!("✅ Socket bound to port 9001");

    // Join multicast group
    match socket.join_multicast_v4("239.255.42.99".parse()?, "0.0.0.0".parse()?) {
        Ok(_) => println!("✅ Joined multicast group 239.255.42.99"),
        Err(e) => println!("❌ Failed to join multicast: {}", e),
    }

    println!("⏰ Waiting for packets (timeout in 30 seconds)...");

    let mut buffer = [0u8; 65536];
    let mut packet_count = 0;

    let start = std::time::Instant::now();

    loop {
        let timeout = tokio::time::timeout(Duration::from_secs(30), socket.recv_from(&mut buffer)).await;

        match timeout {
            Ok(Ok((len, addr))) => {
                packet_count += 1;
                let data = String::from_utf8_lossy(&buffer[..len]);

                println!("📦 Packet #{}: {} bytes from {}", packet_count, len, addr);
                if len < 200 {  // Only print small packets
                    println!("   Data: {}", data);
                } else {
                    println!("   Data: {}... (truncated)", &data[..50]);
                }

                if data.starts_with("TEST_PACKET") {
                    println!("✅ Test packet received successfully!");
                }
            }
            Ok(Err(e)) => {
                println!("❌ Error receiving packet: {}", e);
                break;
            }
            Err(_) => {
                println!("⏰ Timeout - no packets received in 30 seconds");
                break;
            }
        }
    }

    println!("📊 Total packets received: {}", packet_count);
    println!("⏰ Total time: {:.1} seconds", start.elapsed().as_secs_f64());

    Ok(())
}