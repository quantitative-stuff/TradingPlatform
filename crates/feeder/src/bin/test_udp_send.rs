use tokio::net::UdpSocket;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🚀 UDP Test Sender");
    println!("📡 Sending to: 239.255.42.99:9001");

    // Create UDP socket
    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    // Enable broadcast for multicast
    socket.set_broadcast(true)?;

    let target = "239.255.42.99:9001";

    for i in 1..=10 {
        let test_message = format!("TEST_PACKET_{:03}", i);

        match socket.send_to(test_message.as_bytes(), target).await {
            Ok(bytes_sent) => {
                println!("✅ Sent packet {}: {} bytes", i, bytes_sent);
            }
            Err(e) => {
                println!("❌ Failed to send packet {}: {}", i, e);
            }
        }

        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    println!("🏁 Test completed");
    Ok(())
}