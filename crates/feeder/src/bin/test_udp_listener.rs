use std::net::UdpSocket;

fn main() {
    println!("UDP Connection Stats Listener");
    println!("===============================");
    println!("Listening on port 9001 for connection stats...\n");
    
    let socket = UdpSocket::bind("127.0.0.1:9001").expect("Failed to bind UDP socket");
    let mut buf = [0; 65536];
    
    loop {
        match socket.recv_from(&mut buf) {
            Ok((size, addr)) => {
                if let Ok(packet_str) = std::str::from_utf8(&buf[..size]) {
                    // Only show connection stats packets
                    if packet_str.starts_with("CONN|") {
                        let parts: Vec<&str> = packet_str.split('|').collect();
                        if parts.len() >= 6 {
                            println!("[{}] {} -> Connected: {}, Disconnected: {}, Reconnects: {}, Total: {}",
                                chrono::Local::now().format("%H:%M:%S"),
                                parts[1],  // exchange
                                parts[2],  // connected
                                parts[3],  // disconnected
                                parts[4],  // reconnects
                                parts[5]   // total
                            );
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("UDP receive error: {}", e);
            }
        }
    }
}