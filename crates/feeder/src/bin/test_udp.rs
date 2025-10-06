use std::net::UdpSocket;
use std::time::Duration;
use std::thread;

fn main() {
    println!("UDP Test Tool");
    println!("=============");
    
    // Test 1: Try to bind to port 9001
    println!("\nTest 1: Checking if port 9001 is available...");
    match UdpSocket::bind("127.0.0.1:9001") {
        Ok(socket) => {
            println!("✅ Port 9001 is available and bound successfully");
            drop(socket);
        }
        Err(e) => {
            println!("❌ Port 9001 is in use or unavailable: {}", e);
            println!("   Make sure the monitor is running!");
        }
    }
    
    // Test 2: Send test packets
    println!("\nTest 2: Sending test packets to port 9001...");
    let sender = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => {
            println!("✅ Created sender socket on {:?}", s.local_addr());
            s
        }
        Err(e) => {
            println!("❌ Failed to create sender socket: {}", e);
            return;
        }
    };
    
    // Send various test packets
    let test_packets = vec![
        "TEST|Hello from test tool",
        "STATS|100|200|1234567890",
        "EXCHANGE|TestExchange|spot|50|100|1234567890",
        "CONN|TestExchange|5|2|3|10",
    ];
    
    for packet in test_packets {
        match sender.send_to(packet.as_bytes(), "127.0.0.1:9001") {
            Ok(bytes) => println!("✅ Sent: {} ({} bytes)", packet, bytes),
            Err(e) => println!("❌ Failed to send: {}", e),
        }
        thread::sleep(Duration::from_millis(500));
    }
    
    println!("\nTest 3: Creating receiver to test if we can receive on 9001...");
    // Try to receive (this will fail if monitor is running, which is expected)
    thread::spawn(|| {
        match UdpSocket::bind("127.0.0.1:9002") {
            Ok(receiver) => {
                println!("✅ Created test receiver on port 9002");
                let mut buf = [0; 1024];
                receiver.set_read_timeout(Some(Duration::from_secs(2))).ok();
                
                // Send to ourselves
                let test_sender = UdpSocket::bind("0.0.0.0:0").unwrap();
                test_sender.send_to(b"SELF_TEST", "127.0.0.1:9002").ok();
                
                match receiver.recv_from(&mut buf) {
                    Ok((size, addr)) => {
                        println!("✅ Self-test successful: received {} bytes from {}", size, addr);
                    }
                    Err(e) => {
                        println!("❌ Self-test failed: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to create test receiver: {}", e);
            }
        }
    }).join().ok();
    
    println!("\n✨ Test complete!");
    println!("If the monitor is running, check if it received the test packets.");
}