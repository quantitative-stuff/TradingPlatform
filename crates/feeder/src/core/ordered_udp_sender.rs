use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration, Instant};
use crate::core::{TradeData, OrderBookData, BinaryUdpPacket};
use crate::error::Result;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::collections::BinaryHeap;
use std::cmp::{Ordering as CmpOrdering, Reverse};
use once_cell::sync::OnceCell;
use tracing::{info, warn};

const MAX_UDP_PACKET_SIZE: usize = 65507;
const CHUNK_SIZE: usize = 60000;
const DEFAULT_BUFFER_WINDOW_MS: u64 = 0; // Set to 0 to disable re-ordering buffer
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 50; // Check buffer every 50ms

// Wrapper for packets with timestamp for ordering
#[derive(Clone, Debug)]
struct TimestampedPacket {
    timestamp: i64, // Exchange timestamp for ordering
    packet_type: String,
    data: Vec<u8>,
    created_at: Instant, // When packet was created (for timeout)
}

impl PartialEq for TimestampedPacket {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl Eq for TimestampedPacket {}

impl PartialOrd for TimestampedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimestampedPacket {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Reverse ordering for min-heap (earliest timestamp first)
        other.timestamp.cmp(&self.timestamp)
    }
}

pub struct OrderedUdpSender {
    tx: mpsc::UnboundedSender<TimestampedPacket>,
    stats_packets_sent: Arc<AtomicU64>,
}

impl OrderedUdpSender {
    pub fn get_total_packets_sent(&self) -> u64 {
        self.stats_packets_sent.load(Ordering::Relaxed)
    }

    pub async fn new(
        bind_addr: &str, 
        target_addr: &str,
        buffer_window_ms: Option<u64>,
        flush_interval_ms: Option<u64>,
    ) -> Result<Self> {
        let actual_bind_addr = if target_addr.starts_with("239.") || target_addr.starts_with("224.") {
            "0.0.0.0:9004"  // Different port from OptimizedUdpSender (which uses 9002)
        } else {
            bind_addr
        };
        
        let socket = UdpSocket::bind(actual_bind_addr).await?;
        info!("Ordered UDP Sender bound to: {}", actual_bind_addr);
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
        let buffer_window = Duration::from_millis(buffer_window_ms.unwrap_or(DEFAULT_BUFFER_WINDOW_MS));
        let flush_interval = Duration::from_millis(flush_interval_ms.unwrap_or(DEFAULT_FLUSH_INTERVAL_MS));
        
        let (tx, mut rx) = mpsc::unbounded_channel::<TimestampedPacket>();
        
        let stats_packets_sent = Arc::new(AtomicU64::new(0));
        let stats_packets_sent_clone = stats_packets_sent.clone();

        // Spawn the ordered sender task
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            let mut packet_buffer: BinaryHeap<Reverse<TimestampedPacket>> = BinaryHeap::new();
            let mut flush_timer = interval(flush_interval);
            flush_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            
            let mut stats_packets_reordered = 0u64;
            let mut stats_last_log = Instant::now();
            let mut last_sent_timestamp = 0i64;
            
            loop {
                tokio::select! {
                    // Receive new packets
                    Some(packet) = rx.recv() => {
                        // Track if packet is out of order
                        if packet.timestamp < last_sent_timestamp {
                            stats_packets_reordered += 1;
                        }
                        packet_buffer.push(Reverse(packet));
                    }
                    
                    // Periodic flush timer
                    _ = flush_timer.tick() => {
                        let now = Instant::now();
                        let mut packets_to_send = Vec::new();
                        
                        // Extract packets that have been buffered long enough
                        while let Some(Reverse(packet)) = packet_buffer.peek() {
                            if now.duration_since(packet.created_at) >= buffer_window {
                                if let Some(Reverse(packet)) = packet_buffer.pop() {
                                    packets_to_send.push(packet);
                                }
                            } else {
                                break; // Remaining packets are too recent
                            }
                        }
                        
                        // Send packets in timestamp order
                        for packet in packets_to_send {
                            if let Err(e) = Self::send_packet(&socket_clone, &target, &packet.data).await {
                                warn!("Failed to send packet: {}", e);
                            } else {
                                last_sent_timestamp = packet.timestamp;
                                stats_packets_sent_clone.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        
                        // Log statistics periodically
                        if stats_last_log.elapsed() >= Duration::from_secs(30) {
                            let sent_count = stats_packets_sent_clone.load(Ordering::Relaxed);
                            info!(
                                "Ordered UDP Sender stats: {} packets sent, {} reordered ({}%)",
                                sent_count,
                                stats_packets_reordered,
                                if sent_count > 0 {
                                    (stats_packets_reordered * 100) / sent_count
                                } else {
                                    0
                                }
                            );
                            stats_last_log = Instant::now();
                        }
                    }
                    
                    else => {
                        info!("Ordered UDP sender channel closed, exiting");
                        break;
                    }
                }
            }
            
            // Flush remaining packets before exiting
            while let Some(Reverse(packet)) = packet_buffer.pop() {
                let _ = Self::send_packet(&socket_clone, &target, &packet.data).await;
            }
        });
        
        Ok(Self { tx, stats_packets_sent })
    }
    
    async fn send_packet(socket: &UdpSocket, target: &str, data: &[u8]) -> Result<()> {
        if data.len() > MAX_UDP_PACKET_SIZE {
            // Handle chunking for large packets
            let chunks: Vec<_> = data.chunks(CHUNK_SIZE).collect();
            let total_chunks = chunks.len();
            
            for (i, chunk) in chunks.iter().enumerate() {
                let header = format!("CHUNK|{}|{}|", i + 1, total_chunks);
                let mut packet_bytes = header.into_bytes();
                packet_bytes.extend_from_slice(chunk);
                
                socket.send_to(&packet_bytes, target).await?;
                
                if i < total_chunks - 1 {
                    tokio::time::sleep(Duration::from_micros(100)).await;
                }
            }
        } else {
            socket.send_to(data, target).await?;
        }
        Ok(())
    }
    
    pub async fn send_trade(&self, trade: &TradeData) -> Result<()> {
        // Create binary packet for ordering
        let mut binary_packet = BinaryUdpPacket::from_trade(trade);
        let packet_bytes = binary_packet.to_bytes();

        // Extract timestamp from trade data
        let timestamp = trade.timestamp;

        let packet = TimestampedPacket {
            timestamp,
            packet_type: "TRADE".to_string(),
            data: packet_bytes,
            created_at: Instant::now(),
        };
        
        self.tx.send(packet)
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub async fn send_orderbook(&self, orderbook: &OrderBookData) -> Result<()> {
        // Send separate packets for bids and asks for ordering
        let timestamp = orderbook.timestamp;

        if !orderbook.bids.is_empty() {
            let mut bids_packet = BinaryUdpPacket::from_orderbook(orderbook, true);
            let packet_bytes = bids_packet.to_bytes();

            let packet = TimestampedPacket {
                timestamp,
                packet_type: "ORDERBOOK_BIDS".to_string(),
                data: packet_bytes,
                created_at: Instant::now(),
            };

            self.tx.send(packet)
                .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        }

        if !orderbook.asks.is_empty() {
            let mut asks_packet = BinaryUdpPacket::from_orderbook(orderbook, false);
            let packet_bytes = asks_packet.to_bytes();

            let packet = TimestampedPacket {
                timestamp,
                packet_type: "ORDERBOOK_ASKS".to_string(),
                data: packet_bytes,
                created_at: Instant::now(),
            };

            self.tx.send(packet)
                .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        }
        Ok(())
    }
    
    pub async fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        let timestamp = chrono::Local::now().timestamp();
        let packet_data = format!("STATS|{}|{}|{}|{}", 
            exchange, trades, orderbooks, timestamp
        );
        
        let packet = TimestampedPacket {
            timestamp,
            packet_type: "STATS".to_string(),
            data: packet_data.into_bytes(),
            created_at: Instant::now(),
        };
        
        self.tx.send(packet)
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub async fn send_exchange_data(&self, exchange: &str, asset_type: &str, trades: usize, orderbooks: usize) -> Result<()> {
        let timestamp = chrono::Local::now().timestamp();
        let packet_data = format!("EXCHANGE|{}|{}|{}|{}|{}", 
            exchange, asset_type, trades, orderbooks, timestamp
        );
        
        let packet = TimestampedPacket {
            timestamp,
            packet_type: "EXCHANGE".to_string(),
            data: packet_data.into_bytes(),
            created_at: Instant::now(),
        };
        
        self.tx.send(packet)
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
    
    pub async fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize, reconnect_count: usize, total: usize) -> Result<()> {
        // Connection status packets should be sent immediately (no ordering needed)
        // but we'll use current timestamp for consistency
        let timestamp = chrono::Local::now().timestamp();
        let packet_data = format!("CONN|{}|{}|{}|{}|{}", 
            exchange, connected, disconnected, reconnect_count, total
        );
        
        let packet = TimestampedPacket {
            timestamp,
            packet_type: "CONN".to_string(),
            data: packet_data.into_bytes(),
            created_at: Instant::now(),
        };
        
        self.tx.send(packet)
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
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
        let timestamp = chrono::Local::now().timestamp();
        let packet_data = format!("EXCHANGE|{}|{}|{}|{}|{}|{}|{}|{}", 
            exchange, asset_type, trades, orderbooks, 
            connected, disconnected, reconnect_count, timestamp
        );
        
        let packet = TimestampedPacket {
            timestamp,
            packet_type: "EXCHANGE".to_string(),
            data: packet_data.into_bytes(),
            created_at: Instant::now(),
        };
        
        self.tx.send(packet)
            .map_err(|_| crate::error::Error::Connection("UDP channel closed".to_string()))?;
        Ok(())
    }
}

// Global ordered UDP sender instance
pub static GLOBAL_ORDERED_UDP_SENDER: OnceCell<Arc<OrderedUdpSender>> = OnceCell::new();

pub async fn init_global_ordered_udp_sender(
    bind_addr: &str, 
    target_addr: &str,
    buffer_window_ms: Option<u64>,
    flush_interval_ms: Option<u64>,
) -> Result<()> {
    let sender = OrderedUdpSender::new(bind_addr, target_addr, buffer_window_ms, flush_interval_ms).await?;
    GLOBAL_ORDERED_UDP_SENDER.set(Arc::new(sender))
        .map_err(|_| crate::error::Error::Config("Failed to initialize ordered UDP sender".to_string()))?;
    Ok(())
}

pub fn get_ordered_udp_sender() -> Option<Arc<OrderedUdpSender>> {
    GLOBAL_ORDERED_UDP_SENDER.get().cloned()
}