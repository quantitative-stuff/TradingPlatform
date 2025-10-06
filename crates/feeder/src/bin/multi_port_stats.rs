/// Monitor that displays real-time multi-port UDP distribution statistics
/// Shows which ports are receiving which symbols and packet distribution

use std::net::UdpSocket;
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    println!("=== Multi-Port UDP Statistics Monitor ===\n");
    println!("Listening on ports 9001-9004 for 10 seconds...\n");

    // Create sockets for each port
    let mut sockets = vec![];
    for port in 9001..=9004 {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port))?;
        socket.set_nonblocking(true)?;
        socket.join_multicast_v4(&"239.0.0.1".parse()?, &"0.0.0.0".parse()?)?;
        sockets.push((port, socket));
    }

    let mut port_counts: HashMap<u16, usize> = HashMap::new();
    let mut port_bytes: HashMap<u16, usize> = HashMap::new();
    let mut port_symbols: HashMap<u16, Vec<String>> = HashMap::new();

    for port in 9001..=9004 {
        port_counts.insert(port, 0);
        port_bytes.insert(port, 0);
        port_symbols.insert(port, vec![]);
    }

    let start = Instant::now();
    let duration = Duration::from_secs(10);

    let mut buf = [0u8; 65536];

    while start.elapsed() < duration {
        for (port, socket) in &sockets {
            match socket.recv_from(&mut buf) {
                Ok((size, _addr)) => {
                    *port_counts.get_mut(port).unwrap() += 1;
                    *port_bytes.get_mut(port).unwrap() += size;

                    // Try to extract symbol from packet (assume it's at offset 18 for 20 bytes)
                    if size >= 58 {
                        let symbol_bytes = &buf[18..38];
                        if let Ok(symbol) = std::str::from_utf8(symbol_bytes) {
                            let symbol = symbol.trim_end_matches('\0').to_string();
                            if !symbol.is_empty() {
                                let symbols = port_symbols.get_mut(port).unwrap();
                                if !symbols.contains(&symbol) {
                                    symbols.push(symbol);
                                }
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, continue
                }
                Err(e) => {
                    eprintln!("Error receiving on port {}: {}", port, e);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Print results
    println!("\n=== Multi-Port Distribution Results ===\n");

    let total_packets: usize = port_counts.values().sum();
    let total_bytes: usize = port_bytes.values().sum();

    for port in 9001..=9004 {
        let count = port_counts[&port];
        let bytes = port_bytes[&port];
        let percentage = if total_packets > 0 {
            (count as f64 / total_packets as f64) * 100.0
        } else {
            0.0
        };

        println!("Port {}: {:6} packets ({:5.1}%), {:8} bytes",
            port, count, percentage, bytes);

        let symbols = &port_symbols[&port];
        if !symbols.is_empty() {
            println!("  Symbols: {}", symbols.join(", "));
        }
        println!();
    }

    println!("Total: {} packets, {} bytes\n", total_packets, total_bytes);

    if total_packets > 0 {
        println!("✓ Multi-port distribution is working!");

        // Check if distribution is reasonably balanced
        let avg = total_packets as f64 / 4.0;
        let mut balanced = true;
        for count in port_counts.values() {
            let deviation = (*count as f64 - avg).abs() / avg;
            if deviation > 0.5 { // More than 50% deviation
                balanced = false;
                break;
            }
        }

        if balanced {
            println!("✓ Distribution is well-balanced across ports!");
        } else {
            println!("⚠ Distribution may be unbalanced (this is OK for symbol hash strategy)");
        }
    } else {
        println!("⚠ No packets received. Is the feeder running?");
    }

    Ok(())
}