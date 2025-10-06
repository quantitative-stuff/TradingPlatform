// Example multicast UDP receiver for pricing platform
// This shows how to receive UDP packets from the feeder using multicast

use tokio::net::UdpSocket;
use std::net::Ipv4Addr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Bind to port 9001 on all interfaces
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    println!("UDP receiver bound to 0.0.0.0:9001");
    
    // Join the multicast group
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface_addr: Ipv4Addr = "0.0.0.0".parse()?;  // Use any interface
    
    socket.join_multicast_v4(multicast_addr, interface_addr)?;
    println!("Joined multicast group 239.1.1.1");
    
    // Buffer for receiving data
    let mut buf = vec![0u8; 65536];
    
    println!("Listening for multicast packets...");
    
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, addr)) => {
                let data = String::from_utf8_lossy(&buf[..len]);
                
                // Parse the packet type
                if let Some(pipe_pos) = data.find('|') {
                    let packet_type = &data[..pipe_pos];
                    
                    match packet_type {
                        "TRADE" => {
                            println!("Received TRADE from {}: {} bytes", addr, len);
                            // Parse trade JSON here
                        },
                        "ORDERBOOK" => {
                            println!("Received ORDERBOOK from {}: {} bytes", addr, len);
                            // Parse orderbook JSON here
                        },
                        "CONNECTION_STATUS" => {
                            println!("Received CONNECTION_STATUS from {}", addr);
                        },
                        "STATS" => {
                            println!("Received STATS from {}", addr);
                        },
                        _ => {
                            println!("Received {} packet from {}", packet_type, addr);
                        }
                    }
                }
            },
            Err(e) => {
                eprintln!("Error receiving packet: {}", e);
            }
        }
    }
}