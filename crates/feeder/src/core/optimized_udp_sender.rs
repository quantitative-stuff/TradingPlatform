use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use serde_json;
use crate::core::{TradeData, OrderBookData};
use crate::error::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use once_cell::sync::OnceCell;
use tracing::{info, debug};

const MAX_UDP_PACKET_SIZE: usize = 65507;
const CHUNK_SIZE: usize = 60000;
const BATCH_SIZE: usize = 1; // Set to 1 to disable batching
const BATCH_TIMEOUT_MICROS: u64 = 0; // Set to 0 to disable timeout-based batching

// Packet types for the channel
#[derive(Clone)]
pub enum UdpPacket {
    Trade(TradeData),
    OrderBook(OrderBookData),
    Stats { 
        exchange: String, 
        trades: usize, 
        orderbooks: usize 
    },
    ExchangeData {
        exchange: String,
        asset_type: String,
        trades: usize,
        orderbooks: usize,
    },
    ConnectionStatus {
        exchange: String,
        connected: usize,
        disconnected: usize,
        reconnect_count: usize,
        total: usize,
    },
    ExchangeDataWithStatus {
        exchange: String,
        asset_type: String,
        trades: usize,
        orderbooks: usize,
        connected: usize,
        disconnected: usize,
        reconnect_count: usize,
    },
}

pub struct OptimizedUdpSender {
    tx: mpsc::UnboundedSender<UdpPacket>,
}

impl OptimizedUdpSender {
    pub async fn new(bind_addr: &str, target_addr: &str) -> Result<Self> {
        // Setup socket with same multicast configuration as before
        let actual_bind_addr = if target_addr.starts_with("239.") || target_addr.starts_with("224.") {
            "0.0.0.0:9002"
        } else {
            bind_addr
        };
        
        let socket = UdpSocket::bind(actual_bind_addr).await?;
        
        // OPTIMIZATION: Socket configuration is handled at OS level
        // On Windows: netsh int ipv4 set global defaultcurhoplimit=64
        // On Linux: sysctl -w net.core.wmem_max=2097152
        info!("Optimized UDP Sender bound to: {}", actual_bind_addr);
        info!("UDP Target address: {}", target_addr);
        
        // Configure multicast if needed
        let addr_without_port = target_addr.split(':').next().unwrap_or("");
        if addr_without_port.starts_with("239.") || addr_without_port.starts_with("224.") {
            socket.set_multicast_ttl_v4(1)?;
            socket.set_multicast_loop_v4(false)?;
            info!("Multicast configuration complete");
        }
        
        let socket = Arc::new(socket);
        let target = target_addr.to_string();
        
        // Create unbounded channel for zero backpressure
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        // Spawn the single dedicated UDP sender task
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            let mut packet_buffer = Vec::with_capacity(BATCH_SIZE);
            let mut last_flush = Instant::now();
            let batch_timeout = Duration::from_micros(BATCH_TIMEOUT_MICROS);
            
            // Statistics for monitoring
            let mut total_packets_sent = 0u64;
            let mut total_bytes_sent = 0u64;
            let mut last_stats_log = Instant::now();
            
            loop {
                // Try to receive packets without blocking
                let mut packets_received = 0;
                
                // Collect up to BATCH_SIZE packets
                while packets_received < BATCH_SIZE {
                    match rx.try_recv() {
                        Ok(packet) => {
                            packet_buffer.push(packet);
                            packets_received += 1;
                        }
                        Err(_) => break,
                    }
                }
                
                // Check if we should flush
                let should_flush = !packet_buffer.is_empty() && 
                    (packet_buffer.len() >= BATCH_SIZE || last_flush.elapsed() >= batch_timeout);
                
                if should_flush {
                    let bytes_sent = Self::flush_packets(&socket_clone, &target, &mut packet_buffer).await;
                    total_packets_sent += packets_received as u64;
                    total_bytes_sent += bytes_sent as u64;
                    last_flush = Instant::now();
                    
                    // Log statistics every 10 seconds
                    if last_stats_log.elapsed() >= Duration::from_secs(10) {
                        debug!(
                            "UDP Sender stats: {} packets, {} bytes sent", 
                            total_packets_sent, 
                            total_bytes_sent
                        );
                        last_stats_log = Instant::now();
                    }
                } else if packet_buffer.is_empty() {
                    // No packets to send, wait for more
                    match rx.recv().await {
                        Some(packet) => {
                            packet_buffer.push(packet);
                            last_flush = Instant::now();
                        }
                        None => {
                            // Channel closed, exit
                            info!("UDP sender channel closed, exiting");
                            break;
                        }
                    }
                } else {
                    // We have some packets but not enough to flush yet
                    // Brief yield to prevent busy-waiting
                    tokio::task::yield_now().await;
                }
            }
        });
        
        Ok(Self { tx })
    }
    
    // These are now non-async and extremely fast - just channel sends
    // OPTIMIZATION: Avoid cloning by moving ownership through the channel
    pub fn send_trade(&self, trade: TradeData) -> Result<()> {
        self.tx.send(UdpPacket::Trade(trade))
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub fn send_orderbook(&self, orderbook: OrderBookData) -> Result<()> {
        self.tx.send(UdpPacket::OrderBook(orderbook))
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.tx.send(UdpPacket::Stats {
            exchange: exchange.to_string(),
            trades,
            orderbooks,
        }).map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub fn send_exchange_data(&self, exchange: &str, asset_type: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.tx.send(UdpPacket::ExchangeData {
            exchange: exchange.to_string(),
            asset_type: asset_type.to_string(),
            trades,
            orderbooks,
        }).map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize, reconnect_count: usize, total: usize) -> Result<()> {
        self.tx.send(UdpPacket::ConnectionStatus {
            exchange: exchange.to_string(),
            connected,
            disconnected,
            reconnect_count,
            total,
        }).map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub fn send_exchange_data_with_status(&self, 
        exchange: &str, 
        asset_type: &str, 
        trades: usize, 
        orderbooks: usize,
        connected: usize,
        disconnected: usize,
        reconnect_count: usize
    ) -> Result<()> {
        self.tx.send(UdpPacket::ExchangeDataWithStatus {
            exchange: exchange.to_string(),
            asset_type: asset_type.to_string(),
            trades,
            orderbooks,
            connected,
            disconnected,
            reconnect_count,
        }).map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    // Internal method to flush packets with true batching
    async fn flush_packets(socket: &UdpSocket, target: &str, packets: &mut Vec<UdpPacket>) -> usize {
        let mut total_bytes = 0;
        
        // OPTIMIZATION: True packet batching - combine multiple packets into single UDP send
        let mut combined_buffer = Vec::with_capacity(65000);
        let mut current_batch_size = 0;
        const MAX_BATCH_SIZE: usize = 60000; // Leave room for headers
        
        for packet in packets.drain(..) {
            let data = match packet {
                UdpPacket::Trade(ref trade) => {
                    if let Ok(json) = serde_json::to_string(trade) {
                        format!("TRADE|{}", json)
                    } else {
                        continue;
                    }
                },
                UdpPacket::OrderBook(ref orderbook) => {
                    if let Ok(json) = serde_json::to_string(orderbook) {
                        format!("ORDERBOOK|{}", json)
                    } else {
                        continue;
                    }
                },
                UdpPacket::Stats { exchange, trades, orderbooks } => {
                    format!("STATS|{}|{}|{}|{}", 
                        exchange, trades, orderbooks, chrono::Local::now().timestamp())
                },
                UdpPacket::ExchangeData { exchange, asset_type, trades, orderbooks } => {
                    format!("EXCHANGE|{}|{}|{}|{}|{}", 
                        exchange, asset_type, trades, orderbooks, chrono::Local::now().timestamp())
                },
                UdpPacket::ConnectionStatus { exchange, connected, disconnected, reconnect_count, total } => {
                    format!("CONN|{}|{}|{}|{}|{}", 
                        exchange, connected, disconnected, reconnect_count, total)
                },
                UdpPacket::ExchangeDataWithStatus { exchange, asset_type, trades, orderbooks, connected, disconnected, reconnect_count } => {
                    format!("EXCHANGE|{}|{}|{}|{}|{}|{}|{}|{}", 
                        exchange, asset_type, trades, orderbooks, connected, disconnected, reconnect_count, chrono::Local::now().timestamp())
                },
            };
            
            let data_bytes = data.as_bytes();
            
            // Handle large packets with chunking if needed
            if data_bytes.len() > MAX_UDP_PACKET_SIZE {
                // First flush any accumulated batch
                if current_batch_size > 0 {
                    if let Ok(bytes_sent) = socket.send_to(&combined_buffer, target).await {
                        total_bytes += bytes_sent;
                    }
                    combined_buffer.clear();
                    current_batch_size = 0;
                }
                
                // Then send the large packet in chunks
                let chunks: Vec<_> = data_bytes.chunks(CHUNK_SIZE).collect();
                let total_chunks = chunks.len();
                
                for (i, chunk) in chunks.iter().enumerate() {
                    let header = format!("CHUNK|{}|{}|", i + 1, total_chunks);
                    let mut packet_bytes = header.into_bytes();
                    packet_bytes.extend_from_slice(chunk);
                    
                    if let Ok(bytes_sent) = socket.send_to(&packet_bytes, target).await {
                        total_bytes += bytes_sent;
                    }
                    
                    // OPTIMIZATION: Reduced delay between chunks for lower latency
                    if i < total_chunks - 1 {
                        tokio::time::sleep(Duration::from_micros(100)).await;
                    }
                }
            } else {
                // OPTIMIZATION: Batch small packets together
                // Check if adding this packet would exceed batch size
                if current_batch_size + data_bytes.len() + 1 > MAX_BATCH_SIZE {
                    // Flush current batch
                    if current_batch_size > 0 {
                        if let Ok(bytes_sent) = socket.send_to(&combined_buffer, target).await {
                            total_bytes += bytes_sent;
                        }
                        combined_buffer.clear();
                        current_batch_size = 0;
                    }
                }
                
                // Add packet to batch with newline separator
                if current_batch_size > 0 {
                    combined_buffer.push(b'\n');
                    current_batch_size += 1;
                }
                combined_buffer.extend_from_slice(data_bytes);
                current_batch_size += data_bytes.len();
            }
        }
        
        // Flush any remaining packets in the batch
        if current_batch_size > 0 {
            if let Ok(bytes_sent) = socket.send_to(&combined_buffer, target).await {
                total_bytes += bytes_sent;
            }
        }
        
        total_bytes
    }
}

// Global optimized UDP sender instance
pub static GLOBAL_OPTIMIZED_UDP_SENDER: OnceCell<Arc<OptimizedUdpSender>> = OnceCell::new();

pub async fn init_global_optimized_udp_sender(bind_addr: &str, target_addr: &str) -> Result<()> {
    let sender = OptimizedUdpSender::new(bind_addr, target_addr).await?;
    GLOBAL_OPTIMIZED_UDP_SENDER.set(Arc::new(sender))
        .map_err(|_| crate::error::Error::Config("Failed to initialize optimized UDP sender".to_string()))?;
    Ok(())
}

pub fn get_optimized_udp_sender() -> Option<Arc<OptimizedUdpSender>> {
    GLOBAL_OPTIMIZED_UDP_SENDER.get().cloned()
}

// Compatibility wrapper for gradual migration
pub struct UdpSenderCompat {
    optimized: Arc<OptimizedUdpSender>,
}

impl UdpSenderCompat {
    pub fn from_optimized(optimized: Arc<OptimizedUdpSender>) -> Self {
        Self { optimized }
    }
    
    // Make async versions that just call the sync versions
    pub async fn send_trade(&self, trade: &TradeData) -> Result<()> {
        self.optimized.send_trade(trade.clone())
    }
    
    pub async fn send_orderbook(&self, orderbook: &OrderBookData) -> Result<()> {
        self.optimized.send_orderbook(orderbook.clone())
    }
    
    pub async fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.optimized.send_stats(exchange, trades, orderbooks)
    }
    
    pub async fn send_exchange_data(&self, exchange: &str, asset_type: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.optimized.send_exchange_data(exchange, asset_type, trades, orderbooks)
    }
    
    pub async fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize, reconnect_count: usize, total: usize) -> Result<()> {
        self.optimized.send_connection_status(exchange, connected, disconnected, reconnect_count, total)
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
        self.optimized.send_exchange_data_with_status(exchange, asset_type, trades, orderbooks, connected, disconnected, reconnect_count)
    }
}