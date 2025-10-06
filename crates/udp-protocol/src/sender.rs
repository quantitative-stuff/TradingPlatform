use tokio::net::UdpSocket;
use serde_json;
use market_types::{Trade, OrderBookUpdate};
use anyhow::Result;
use std::sync::Arc;
use once_cell::sync::OnceCell;

const MAX_UDP_PACKET_SIZE: usize = 65507; // Maximum UDP packet size (65535 - 20 IP header - 8 UDP header)
const CHUNK_SIZE: usize = 60000; // Safe chunk size to avoid fragmentation

pub struct UdpSender {
    socket: Arc<UdpSocket>,
    target_addr: String,
}

impl UdpSender {
    pub async fn new(bind_addr: &str, target_addr: &str) -> Result<Self> {
        // For multicast, we should bind to a specific port on all interfaces
        // This ensures the sender can send from a consistent source
        let actual_bind_addr = if target_addr.starts_with("239.") || target_addr.starts_with("224.") {
            "0.0.0.0:9002"  // Use a specific port for multicast sending
        } else {
            bind_addr
        };
        
        let socket = UdpSocket::bind(actual_bind_addr).await?;
        println!("UDP Sender bound to: {}", actual_bind_addr);
        println!("UDP Target address: {}", target_addr);
        
        // Parse the target address to check if it's multicast
        let addr_without_port = target_addr.split(':').next().unwrap_or("");
        
        // Configure for multicast if target is a multicast address
        if addr_without_port.starts_with("239.") || addr_without_port.starts_with("224.") {
            println!("Configuring for multicast address: {}", target_addr);
            
            // Set multicast TTL 
            // 1 = same subnet only
            // 2 = can cross one router
            // We use 1 since you mentioned same switch
            socket.set_multicast_ttl_v4(1)?;
            println!("Set multicast TTL to 1 (same subnet)");
            
            // Disable multicast loop - we don't need to receive our own packets
            socket.set_multicast_loop_v4(false)?;
            println!("Disabled multicast loop");
            
            // Note: tokio::net::UdpSocket doesn't support set_multicast_if_v4
            // The OS will automatically select the appropriate interface for multicast
            println!("Multicast configuration complete");
        } else {
            println!("Using unicast for address: {}", target_addr);
        }
        
        Ok(Self {
            socket: Arc::new(socket),
            target_addr: target_addr.to_string(),
        })
    }

    pub async fn send_trade(&self, trade: &Trade) -> Result<()> {
        let json = serde_json::to_string(trade)?;
        let packet = format!("TRADE|{}", json);
        self.send_packet(packet.as_bytes()).await
    }

    pub async fn send_orderbook(&self, orderbook: &OrderBookUpdate) -> Result<()> {
        let json = serde_json::to_string(orderbook)?;
        let packet = format!("ORDERBOOK|{}", json);
        
        // Check if packet is too large
        if packet.len() > CHUNK_SIZE {
            // For large orderbooks, send in chunks
            self.send_chunked_packet(&packet).await
        } else {
            self.send_packet(packet.as_bytes()).await
        }
    }

    async fn send_packet(&self, data: &[u8]) -> Result<()> {
        if data.len() > MAX_UDP_PACKET_SIZE {
            return self.send_chunked_packet(std::str::from_utf8(data).unwrap_or("")).await;
        }
        
        let bytes_sent = self.socket.send_to(data, &self.target_addr).await?;
        
        // Debug: Log first packet of each type
        static LOGGED_TRADE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static LOGGED_ORDERBOOK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        
        let data_str = std::str::from_utf8(data).unwrap_or("");
        if data_str.starts_with("TRADE") && !LOGGED_TRADE.load(std::sync::atomic::Ordering::Relaxed) {
            println!("Sent first TRADE packet: {} bytes to {}", bytes_sent, self.target_addr);
            LOGGED_TRADE.store(true, std::sync::atomic::Ordering::Relaxed);
        } else if data_str.starts_with("ORDERBOOK") && !LOGGED_ORDERBOOK.load(std::sync::atomic::Ordering::Relaxed) {
            println!("Sent first ORDERBOOK packet: {} bytes to {}", bytes_sent, self.target_addr);
            LOGGED_ORDERBOOK.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        
        Ok(())
    }

    async fn send_chunked_packet(&self, data: &str) -> Result<()> {
        let chunks = data.as_bytes().chunks(CHUNK_SIZE);
        let total_chunks = chunks.len();
        
        for (i, chunk) in chunks.enumerate() {
            let header = format!("CHUNK|{}|{}|", i + 1, total_chunks);
            let mut packet = header.into_bytes();
            packet.extend_from_slice(chunk);
            
            self.socket.send_to(&packet, &self.target_addr).await?;
            
            // Small delay between chunks to avoid overwhelming the receiver
            if i < total_chunks - 1 {
                tokio::time::sleep(tokio::time::Duration::from_micros(100)).await;
            }
        }
        
        Ok(())
    }

    pub async fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        let packet = format!("STATS|{}|{}|{}|{}", 
            exchange,
            trades, 
            orderbooks,
            chrono::Local::now().timestamp()
        );
        self.send_packet(packet.as_bytes()).await
    }
    
    pub async fn send_exchange_data(&self, exchange: &str, asset_type: &str, trades: usize, orderbooks: usize) -> Result<()> {
        let packet = format!("EXCHANGE|{}|{}|{}|{}|{}", 
            exchange,
            asset_type,
            trades, 
            orderbooks,
            chrono::Local::now().timestamp()
        );
        self.send_packet(packet.as_bytes()).await
    }
    
    pub async fn send_exchange_data_with_status(&self, 
        exchange: &str, 
        asset_type: &str, 
        trades: usize, 
        orderbooks: usize,
        connected: usize,
        disconnected: usize,
        reconnect_count: usize
    ) -> Result<()> {
        let packet = format!("EXCHANGE|{}|{}|{}|{}|{}|{}|{}|{}", 
            exchange,
            asset_type,
            trades, 
            orderbooks,
            connected,
            disconnected,
            reconnect_count,
            chrono::Local::now().timestamp()
        );
        self.send_packet(packet.as_bytes()).await
    }
    
    pub async fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize, reconnect_count: usize, total: usize) -> Result<()> {
        let packet = format!("CONN|{}|{}|{}|{}|{}", 
            exchange,
            connected,
            disconnected,
            reconnect_count,
            total
        );
        self.send_packet(packet.as_bytes()).await
    }
}

// Global UDP sender instance
pub static GLOBAL_UDP_SENDER: OnceCell<Arc<UdpSender>> = OnceCell::new();

pub async fn init_global_udp_sender(bind_addr: &str, target_addr: &str) -> Result<()> {
    let sender = UdpSender::new(bind_addr, target_addr).await?;
    GLOBAL_UDP_SENDER.set(Arc::new(sender))
        .map_err(|_| anyhow::anyhow!("Failed to initialize global UDP sender"))?;
    Ok(())
}

pub fn get_udp_sender() -> Option<Arc<UdpSender>> {
    GLOBAL_UDP_SENDER.get().cloned()
}