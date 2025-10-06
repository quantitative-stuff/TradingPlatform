use anyhow::Result;
use std::net::UdpSocket;
use std::time::Instant;
use tracing::{info, error};
use std::collections::HashMap;

// Stock data multicast endpoint - matches stock_feeder.rs
const STOCK_MULTICAST_ADDR: &str = "239.255.42.100";
const STOCK_MULTICAST_PORT: u16 = 9002;

struct StockMonitor {
    socket: UdpSocket,
    stats: HashMap<String, SymbolStats>,
    start_time: Instant,
    total_messages: u64,
}

struct SymbolStats {
    packet_count: u64,
    last_update: Instant,
}

impl StockMonitor {
    fn new() -> Result<Self> {
        // Create UDP socket - try binding to any available port first, then specific port
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", STOCK_MULTICAST_PORT)) {
            Ok(s) => s,
            Err(e) => {
                println!("Port {} is in use ({}), trying alternative port...", STOCK_MULTICAST_PORT, e);
                // Try binding to any available port
                UdpSocket::bind("0.0.0.0:0")?
            }
        };
        
        // Join multicast group
        let multicast_addr: std::net::Ipv4Addr = STOCK_MULTICAST_ADDR.parse()?;
        let interface = std::net::Ipv4Addr::new(0, 0, 0, 0);
        socket.join_multicast_v4(&multicast_addr, &interface)?;
        
        // Set non-blocking
        socket.set_nonblocking(true)?;
        
        Ok(Self {
            socket,
            stats: HashMap::new(),
            start_time: Instant::now(),
            total_messages: 0,
        })
    }
    
    async fn run(&mut self) -> Result<()> {
        info!("Stock UDP Monitor started on {}:{}", STOCK_MULTICAST_ADDR, STOCK_MULTICAST_PORT);
        info!("Listening for stock market data...");
        
        let mut buf = vec![0u8; 65536];
        let mut last_display = Instant::now();
        
        loop {
            // Try to receive data
            match self.socket.recv(&mut buf) {
                Ok(size) => {
                    self.total_messages += 1;
                    
                    // For now, just count packets per symbol
                    // In a real implementation, you'd parse the binary protocol
                    if size > 0 {
                        // Try to extract symbol from packet (simplified)
                        let packet_str = String::from_utf8_lossy(&buf[..size.min(100)]);
                        
                        // Update stats for "STOCK" as placeholder
                        let symbol = if packet_str.contains("005930") {
                            "005930"
                        } else if packet_str.contains("000660") {
                            "000660"
                        } else {
                            "OTHER"
                        };
                        
                        let stats = self.stats.entry(symbol.to_string()).or_insert(SymbolStats {
                            packet_count: 0,
                            last_update: Instant::now(),
                        });
                        stats.packet_count += 1;
                        stats.last_update = Instant::now();
                    }
                    
                    // Display stats every second
                    if last_display.elapsed().as_secs() >= 1 {
                        self.display_stats();
                        last_display = Instant::now();
                    }
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        error!("Socket error: {}", e);
                    }
                }
            }
            
            // Small delay to prevent busy loop
            tokio::time::sleep(tokio::time::Duration::from_micros(100)).await;
        }
    }
    
    fn display_stats(&self) {
        // Clear screen
        print!("\x1B[2J\x1B[1;1H");
        
        println!("╔════════════════════════════════════════════════════════════════╗");
        println!("║                    STOCK UDP MONITOR                            ║");
        println!("╠════════════════════════════════════════════════════════════════╣");
        println!("║ Multicast: {}:{}                                      ║", STOCK_MULTICAST_ADDR, STOCK_MULTICAST_PORT);
        println!("║ Runtime: {:.0}s                                                 ║", self.start_time.elapsed().as_secs());
        println!("║ Total Packets: {:8}                                         ║", self.total_messages);
        println!("╠════════════════════════════════════════════════════════════════╣");
        
        if self.stats.is_empty() {
            println!("║ No data received yet...                                         ║");
            println!("║ Make sure stock_feeder is running!                              ║");
        } else {
            println!("║ Symbol     │ Packets    │ Last Update                          ║");
            println!("╠════════════════════════════════════════════════════════════════╣");
            
            // Sort symbols for consistent display
            let mut symbols: Vec<_> = self.stats.keys().collect();
            symbols.sort();
            
            for symbol in symbols.iter().take(10) {
                if let Some(stats) = self.stats.get(*symbol) {
                    let age = stats.last_update.elapsed().as_secs();
                    println!("║ {:10} │ {:10} │ {}s ago                               ║",
                        symbol,
                        stats.packet_count,
                        age
                    );
                }
            }
        }
        
        println!("╚════════════════════════════════════════════════════════════════╝");
        println!("\nPress Ctrl+C to exit");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    
    println!("Starting Stock UDP Monitor...");
    println!("This will show data being broadcast from stock_feeder");
    
    let mut monitor = StockMonitor::new()?;
    
    // Handle Ctrl+C
    tokio::select! {
        result = monitor.run() => {
            if let Err(e) = result {
                error!("Monitor error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("\nShutting down stock monitor...");
        }
    }
    
    Ok(())
}