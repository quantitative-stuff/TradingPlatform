use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔍 MULTICAST DEBUG TOOL".bright_cyan().bold());
    println!("{}", "=".repeat(50).bright_blue());
    
    // Test 1: Try binding to the port
    println!("\n1️⃣ Testing UDP bind on port 9001...");
    match UdpSocket::bind("0.0.0.0:9001").await {
        Ok(socket) => {
            println!("   ✅ Successfully bound to 0.0.0.0:9001");
            
            // Test 2: Try joining multicast
            println!("\n2️⃣ Testing multicast join...");
            let multicast_addr: Ipv4Addr = "239.255.0.1".parse()?;
            let interface = Ipv4Addr::new(0, 0, 0, 0);
            
            match socket.join_multicast_v4(multicast_addr, interface) {
                Ok(_) => {
                    println!("   ✅ Successfully joined multicast 239.255.0.1");
                    
                    // Test 3: Try receiving ANY packet
                    println!("\n3️⃣ Waiting for packets (10 seconds)...");
                    println!("   (Make sure feeder is running!)");
                    
                    let mut buf = vec![0u8; 65536];
                    let mut packet_count = 0;
                    
                    for i in 0..10 {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(1),
                            socket.recv_from(&mut buf)
                        ).await {
                            Ok(Ok((len, addr))) => {
                                packet_count += 1;
                                println!("   📦 Packet #{} from {} ({} bytes)", packet_count, addr, len);
                                
                                // Show first 100 bytes of first packet
                                if packet_count == 1 {
                                    let preview = if len > 100 { &buf[..100] } else { &buf[..len] };
                                    if let Ok(text) = std::str::from_utf8(preview) {
                                        println!("      Preview: {}", text.bright_yellow());
                                    } else {
                                        println!("      Binary packet (not text)");
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                println!("   ❌ Error receiving: {}", e);
                            }
                            Err(_) => {
                                print!("   ⏳ Second {}/10: No packets", i+1);
                                if packet_count > 0 {
                                    println!(" (received {} so far)", packet_count);
                                } else {
                                    println!();
                                }
                            }
                        }
                    }
                    
                    if packet_count == 0 {
                        println!("\n   ⚠️ No packets received!");
                        println!("\n   Possible issues:");
                        println!("   1. Feeder not running");
                        println!("   2. Feeder sending to different address/port");
                        println!("   3. Firewall blocking UDP");
                        println!("   4. Network interface issues");
                    } else {
                        println!("\n   ✅ Received {} packets total", packet_count);
                    }
                }
                Err(e) => {
                    println!("   ❌ Failed to join multicast: {}", e);
                }
            }
        }
        Err(e) => {
            println!("   ❌ Failed to bind: {}", e);
            println!("   Another process might be using port 9001");
        }
    }
    
    // Test 4: Try alternative - direct UDP (non-multicast)
    println!("\n4️⃣ Testing direct UDP on localhost:9001...");
    match UdpSocket::bind("127.0.0.1:9002").await {
        Ok(socket) => {
            println!("   ✅ Bound to 127.0.0.1:9002 for testing");
            
            // Send a test packet to ourselves via localhost
            let test_msg = b"TEST|self_test|123";
            match socket.send_to(test_msg, "127.0.0.1:9002").await {
                Ok(bytes) => {
                    println!("   📤 Sent {} bytes to self", bytes);
                    
                    let mut buf = vec![0u8; 1024];
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        socket.recv_from(&mut buf)
                    ).await {
                        Ok(Ok((len, addr))) => {
                            println!("   📦 Received {} bytes from {}", len, addr);
                            println!("   ✅ Local UDP works!");
                        }
                        _ => {
                            println!("   ⚠️ Couldn't receive own packet");
                        }
                    }
                }
                Err(e) => {
                    println!("   ❌ Send failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("   ❌ Bind failed: {}", e);
        }
    }
    
    // Test 5: Check what address the feeder is actually using
    println!("\n5️⃣ Check feeder configuration:");
    println!("   Look in your feeder code for:");
    println!("   - init_global_udp_sender() call");
    println!("   - Target address (should be 239.255.0.1:9001)");
    println!("   - Or might be using localhost (127.0.0.1:9001)");
    
    Ok(())
}