use anyhow::Result;
use std::net::UdpSocket;
use std::time::Instant;
use std::collections::HashMap;

// Stock data multicast endpoint - matches stock_feeder.rs
const STOCK_MULTICAST_ADDR: &str = "239.255.42.100";
const STOCK_MULTICAST_PORT: u16 = 9002;

// Use a different port for receiving to avoid conflicts
const RECEIVER_PORT: u16 = 9102;  // Different from sender port

struct StockReceiver {
    socket: UdpSocket,
    stats: HashMap<String, u64>,
    start_time: Instant,
    total_packets: u64,
    last_display: Instant,
}

impl StockReceiver {
    fn new() -> Result<Self> {
        println!("Creating UDP receiver on port {}...", STOCK_MULTICAST_PORT);
        
        // Bind to the SAME port as the multicast to receive the data
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", STOCK_MULTICAST_PORT))?;
        
        // Join the multicast group
        let multicast_addr: std::net::Ipv4Addr = STOCK_MULTICAST_ADDR.parse()?;
        let interface = std::net::Ipv4Addr::new(0, 0, 0, 0);
        socket.join_multicast_v4(&multicast_addr, &interface)?;
        
        // Set non-blocking mode
        socket.set_nonblocking(true)?;
        
        println!("Successfully joined multicast group {}:{}", STOCK_MULTICAST_ADDR, STOCK_MULTICAST_PORT);
        
        Ok(Self {
            socket,
            stats: HashMap::new(),
            start_time: Instant::now(),
            total_packets: 0,
            last_display: Instant::now(),
        })
    }
    
    fn run(&mut self) -> Result<()> {
        println!("\n╔════════════════════════════════════════════════════════════════╗");
        println!("║              STOCK DATA RECEIVER - Listening...                 ║");
        println!("╚════════════════════════════════════════════════════════════════╝");
        
        let mut buf = vec![0u8; 65536];
        
        loop {
            // Try to receive data
            match self.socket.recv_from(&mut buf) {
                Ok((size, addr)) => {
                    self.total_packets += 1;
                    
                    // Parse packet to identify symbol (simplified)
                    let data_str = String::from_utf8_lossy(&buf[..size.min(200)]);
                    
                    // Look for known symbols in the data
                    let symbol = if data_str.contains("005930") {
                        "005930-Samsung"
                    } else if data_str.contains("000660") {
                        "000660-SKHynix"
                    } else if data_str.contains("005380") {
                        "005380-Hyundai"
                    } else if data_str.contains("035420") {
                        "035420-NAVER"
                    } else if data_str.contains("000270") {
                        "000270-Kia"
                    } else {
                        "Other"
                    };
                    
                    *self.stats.entry(symbol.to_string()).or_insert(0) += 1;
                    
                    // Show first few packets in detail
                    if self.total_packets <= 5 {
                        println!("Packet #{}: {} bytes from {}", self.total_packets, size, addr);
                        println!("  Preview: {}", &data_str[..data_str.len().min(100)]);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, this is normal
                }
                Err(e) => {
                    println!("Socket error: {}", e);
                }
            }
            
            // Display stats every 2 seconds
            if self.last_display.elapsed().as_secs() >= 2 {
                self.display_stats();
                self.last_display = Instant::now();
            }
            
            // Small sleep to prevent CPU spinning
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    
    fn display_stats(&self) {
        println!("\n═══════════════════════════════════════════════════════");
        println!("Runtime: {:.0}s | Total Packets: {}", 
            self.start_time.elapsed().as_secs(), 
            self.total_packets);
        
        if self.total_packets == 0 {
            println!("No data received yet. Make sure stock_feeder is running!");
        } else {
            println!("Packets by Symbol:");
            let mut sorted_stats: Vec<_> = self.stats.iter().collect();
            sorted_stats.sort_by_key(|&(k, _)| k);
            
            for (symbol, count) in sorted_stats {
                println!("  {} : {} packets", symbol, count);
            }
        }
        println!("═══════════════════════════════════════════════════════");
    }
}

fn main() -> Result<()> {
    println!("Stock Data Receiver");
    println!("===================");
    println!("This receives UDP multicast data from stock_feeder");
    println!("Make sure stock_feeder is running in another terminal!");
    
    let mut receiver = StockReceiver::new()?;
    
    // Run the receiver
    if let Err(e) = receiver.run() {
        println!("Error: {}", e);
    }
    
    Ok(())
}