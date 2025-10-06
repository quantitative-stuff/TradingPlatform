use std::net::UdpSocket as StdUdpSocket;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use std::time::Duration;
use tracing::{info, warn, error};
use socket2::{Socket, Domain, Type, Protocol};

/// Enhanced UDP sender with proper socket buffer configuration
/// Based on recommendations from docs/Data_Inversion.md
pub struct BufferedUdpSender {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl BufferedUdpSender {
    pub async fn new(target_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        
        // Create socket with explicit buffer configuration
        let socket = create_optimized_udp_socket()?;
        
        // Convert to tokio UdpSocket
        let tokio_socket = UdpSocket::from_std(socket)?;
        
        // Parse target address
        let target: std::net::SocketAddr = target_addr.parse()?;
        
        // Check if multicast
        let is_multicast = target_addr.starts_with("239.") || target_addr.starts_with("224.");
        
        if is_multicast {
            // Configure multicast
            let sock_ref = socket2::SockRef::from(&tokio_socket);
            // Set multicast TTL to 1 (local network only)
            sock_ref.set_multicast_ttl_v4(1)?;
            // Disable loopback
            sock_ref.set_multicast_loop_v4(false)?;
            info!("Configured multicast for {}", target_addr);
        }
        
        // Spawn sender task
        tokio::spawn(async move {
            let mut packet_count = 0u64;
            let mut error_count = 0u64;
            let mut last_log = std::time::Instant::now();
            
            while let Some(data) = rx.recv().await {
                // Enforce MTU limit to prevent fragmentation
                if data.len() > 1400 {
                    warn!("Packet size {} exceeds recommended MTU, may fragment", data.len());
                }
                
                match tokio_socket.send_to(&data, &target).await {
                    Ok(bytes_sent) => {
                        packet_count += 1;
                        
                        if bytes_sent != data.len() {
                            warn!("Partial send: {} of {} bytes", bytes_sent, data.len());
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        
                        // Check for buffer overflow
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            warn!("Socket buffer full, packet dropped");
                            // Small delay to let buffer drain
                            tokio::time::sleep(Duration::from_micros(100)).await;
                        } else {
                            error!("Failed to send packet: {}", e);
                        }
                    }
                }
                
                // Periodic stats
                if last_log.elapsed().as_secs() >= 60 {
                    info!(
                        "UDP stats: {} packets sent, {} errors ({:.3}% loss)",
                        packet_count,
                        error_count,
                        (error_count as f64 / (packet_count + error_count) as f64) * 100.0
                    );
                    last_log = std::time::Instant::now();
                }
            }
            
            info!("UDP sender task terminated");
        });
        
        Ok(Self { tx })
    }
    
    /// Send packet with size validation
    pub fn send(&self, data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        // Validate packet size
        if data.len() > 65507 {
            return Err("Packet exceeds maximum UDP size (65507 bytes)".into());
        }
        
        // Recommend smaller packets
        if data.len() > 1400 {
            warn!("Large packet ({} bytes) may fragment. Consider keeping under 1400 bytes", data.len());
        }
        
        self.tx.send(data)
            .map_err(|_| "UDP sender channel closed".into())
    }
    
    /// Send with automatic chunking for large data
    pub fn send_chunked(&self, data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        const CHUNK_SIZE: usize = 1200; // Conservative size to avoid fragmentation
        
        if data.len() <= CHUNK_SIZE {
            return self.send(data);
        }
        
        // Split into chunks
        let total_chunks = (data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
        
        for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
            // Add chunk header
            let header = format!("CHUNK|{}|{}|", i + 1, total_chunks);
            let mut packet = header.into_bytes();
            packet.extend_from_slice(chunk);
            
            self.send(packet)?;
            
            // Small delay between chunks to prevent overwhelming
            std::thread::sleep(Duration::from_micros(100));
        }
        
        Ok(())
    }
}

/// Create UDP socket with optimized buffer settings
fn create_optimized_udp_socket() -> Result<StdUdpSocket, Box<dyn std::error::Error>> {
    // Create raw socket
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    
    // Set socket options BEFORE binding
    
    // 1. Increase send buffer (default is often 64KB)
    match socket.set_send_buffer_size(4 * 1024 * 1024) {  // 4MB
        Ok(_) => info!("Set UDP send buffer to 4MB"),
        Err(e) => warn!("Could not set send buffer size: {}", e),
    }
    
    // 2. Increase receive buffer (important for receivers)
    match socket.set_recv_buffer_size(8 * 1024 * 1024) {  // 8MB
        Ok(_) => info!("Set UDP receive buffer to 8MB"),
        Err(e) => warn!("Could not set receive buffer size: {}", e),
    }
    
    // 3. Set non-blocking mode
    socket.set_nonblocking(true)?;
    
    // 4. Reuse address (useful for multicast)
    socket.set_reuse_address(true)?;
    
    // Log actual buffer sizes
    if let Ok(send_size) = socket.send_buffer_size() {
        info!("Actual UDP send buffer: {} bytes", send_size);
    }
    if let Ok(recv_size) = socket.recv_buffer_size() {
        info!("Actual UDP receive buffer: {} bytes", recv_size);
    }
    
    // Bind to any available port
    let addr = "0.0.0.0:0".parse::<std::net::SocketAddr>()?;
    socket.bind(&addr.into())?;
    
    // Convert to standard UdpSocket
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_packet_size_validation() {
        let sender = BufferedUdpSender::new("127.0.0.1:9999").await.unwrap();
        
        // Test normal packet
        let normal = vec![0u8; 1000];
        assert!(sender.send(normal).is_ok());
        
        // Test oversized packet
        let oversized = vec![0u8; 70000];
        assert!(sender.send(oversized).is_err());
        
        // Test chunking
        let large = vec![0u8; 5000];
        assert!(sender.send_chunked(large).is_ok());
    }
}