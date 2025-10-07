use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use crate::core::{TradeData, OrderBookData};
use crate::error::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use once_cell::sync::OnceCell;
use tracing::{info, debug, warn};

/// Maximum UDP packet size (65507 bytes theoretical, but we stay under MTU)
const MAX_UDP_PACKET_SIZE: usize = 1472; // Safe MTU size
const BATCH_SIZE: usize = 50; // Number of packets to batch together
const BATCH_TIMEOUT_MICROS: u64 = 100; // Microseconds to wait for batching

/// Packet types for the binary channel
#[derive(Clone)]
pub enum BinaryUdpMessage {
    Trade(TradeData),
    OrderBook(OrderBookData),
    Stats {
        exchange: String,
        trades: usize,
        orderbooks: usize
    },
    ConnectionStatus {
        exchange: String,
        connected: usize,
        disconnected: usize,
        reconnect_count: usize,
        total: usize,
    },
}

/// High-performance binary UDP sender using the binary packet format
pub struct BinaryUdpSender {
    tx: mpsc::UnboundedSender<BinaryUdpMessage>,
}

impl BinaryUdpSender {
    /// Create new binary UDP sender
    pub async fn new(bind_addr: &str, target_addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind(bind_addr).await?;

        // Configure socket for multicast if needed
        if target_addr.starts_with("239.") {
            // Multicast setup
            socket.set_broadcast(true)?;
            info!("Configured for multicast to {}", target_addr);
        }

        let socket = Arc::new(socket);
        let target = target_addr.to_string();
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Spawn the binary packet processing task
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            let mut packet_buffer: Vec<BinaryUdpMessage> = Vec::with_capacity(BATCH_SIZE);
            let mut last_flush = Instant::now();

            info!("Binary UDP sender task started for {}", target);

            loop {
                // Try to receive packets with timeout for batching
                let timeout_duration = Duration::from_micros(BATCH_TIMEOUT_MICROS);
                let packet_option = tokio::time::timeout(timeout_duration, rx.recv()).await;

                match packet_option {
                    Ok(Some(packet)) => {
                        packet_buffer.push(packet);

                        // Flush if buffer is full or enough time has passed
                        if packet_buffer.len() >= BATCH_SIZE ||
                           last_flush.elapsed() > Duration::from_micros(BATCH_TIMEOUT_MICROS * 10) {

                            Self::flush_binary_packets(&socket_clone, &target, &mut packet_buffer).await;
                            last_flush = Instant::now();
                        }
                    }
                    Ok(None) => {
                        // Channel closed
                        info!("Binary UDP sender channel closed, exiting");
                        break;
                    }
                    Err(_) => {
                        // Timeout - flush any pending packets
                        if !packet_buffer.is_empty() {
                            Self::flush_binary_packets(&socket_clone, &target, &mut packet_buffer).await;
                            last_flush = Instant::now();
                        }
                    }
                }
            }
        });

        Ok(BinaryUdpSender {
            tx,
        })
    }

    /// Send trade data as binary packet
    pub fn send_trade(&self, trade: TradeData) -> Result<()> {
        self.tx.send(BinaryUdpMessage::Trade(trade))
            .map_err(|_| crate::error::Error::Connection("Binary UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Send orderbook data as binary packets (separate for bids and asks)
    pub fn send_orderbook(&self, orderbook: OrderBookData) -> Result<()> {
        self.tx.send(BinaryUdpMessage::OrderBook(orderbook))
            .map_err(|_| crate::error::Error::Connection("Binary UDP channel closed".to_string()))?;
        Ok(())
    }

    // Overloaded helpers - accept either struct or individual parameters
    pub fn send_trade_data(&self, trade: TradeData) -> Result<()> {
        self.send_trade(trade)
    }

    pub fn send_orderbook_data(&self, orderbook: OrderBookData) -> Result<()> {
        self.send_orderbook(orderbook)
    }

    /// Send stats as text packet (for compatibility with monitoring)
    pub fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.tx.send(BinaryUdpMessage::Stats {
            exchange: exchange.to_string(),
            trades,
            orderbooks,
        }).map_err(|_| crate::error::Error::Connection("Binary UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Send connection status as text packet (for compatibility with monitoring)
    pub fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize, reconnect_count: usize, total: usize) -> Result<()> {
        self.tx.send(BinaryUdpMessage::ConnectionStatus {
            exchange: exchange.to_string(),
            connected,
            disconnected,
            reconnect_count,
            total,
        }).map_err(|_| crate::error::Error::Connection("Binary UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Flush binary packets to UDP socket
    async fn flush_binary_packets(
        socket: &UdpSocket,
        target: &str,
        packets: &mut Vec<BinaryUdpMessage>
    ) {
        let mut bytes_sent = 0;

        for packet in packets.drain(..) {
            match packet {
                BinaryUdpMessage::Trade(trade) => {
                    println!("UDP Sender: Processing trade for {} @ {} x {}",
                         trade.symbol, trade.price, trade.quantity);
                    // Create binary trade packet
                    let mut binary_packet = crate::core::BinaryUdpPacket::from_trade(&trade);
                    let packet_bytes = binary_packet.to_bytes();

                    if packet_bytes.len() <= MAX_UDP_PACKET_SIZE {
                        match socket.send_to(&packet_bytes, target).await {
                            Ok(sent) => {
                                bytes_sent += sent;
                                debug!("Sent binary trade packet: {} bytes", sent);
                            }
                            Err(e) => warn!("Failed to send binary trade packet: {}", e),
                        }
                    } else {
                        warn!("Binary trade packet too large: {} bytes", packet_bytes.len());
                    }
                }
                BinaryUdpMessage::OrderBook(orderbook) => {
                    println!("UDP Sender: Processing orderbook for {} with {} bids and {} asks",
                         orderbook.symbol, orderbook.bids.len(), orderbook.asks.len());
                    // Send separate packets for bids and asks
                    if !orderbook.bids.is_empty() {
                        let mut bids_packet = crate::core::BinaryUdpPacket::from_orderbook(&orderbook, true);
                        let packet_bytes = bids_packet.to_bytes();

                        if packet_bytes.len() <= MAX_UDP_PACKET_SIZE {
                            match socket.send_to(&packet_bytes, target).await {
                                Ok(sent) => {
                                    bytes_sent += sent;
                                    info!("Sent binary orderbook bids packet: {} bytes for {}", sent, orderbook.symbol);
                                }
                                Err(e) => warn!("Failed to send binary bids packet: {}", e),
                            }
                        }
                    }

                    if !orderbook.asks.is_empty() {
                        let mut asks_packet = crate::core::BinaryUdpPacket::from_orderbook(&orderbook, false);
                        let packet_bytes = asks_packet.to_bytes();

                        if packet_bytes.len() <= MAX_UDP_PACKET_SIZE {
                            match socket.send_to(&packet_bytes, target).await {
                                Ok(sent) => {
                                    bytes_sent += sent;
                                    info!("Sent binary orderbook asks packet: {} bytes for {}", sent, orderbook.symbol);
                                }
                                Err(e) => warn!("Failed to send binary asks packet: {}", e),
                            }
                        }
                    } else {
                        warn!("No asks to send for {}", orderbook.symbol);
                    }
                }
                BinaryUdpMessage::Stats { exchange, trades, orderbooks } => {
                    // Send stats as text for monitoring compatibility
                    let stats_msg = format!("STATS|{}|{}|{}|{}",
                        exchange, trades, orderbooks, chrono::Local::now().timestamp());

                    match socket.send_to(stats_msg.as_bytes(), target).await {
                        Ok(sent) => {
                            bytes_sent += sent;
                            debug!("Sent stats packet: {} bytes", sent);
                        }
                        Err(e) => warn!("Failed to send stats packet: {}", e),
                    }
                }
                BinaryUdpMessage::ConnectionStatus { exchange, connected, disconnected, reconnect_count, total } => {
                    // Send connection status as text for monitoring compatibility
                    let conn_msg = format!("CONN|{}|{}|{}|{}|{}",
                        exchange, connected, disconnected, reconnect_count, total);

                    match socket.send_to(conn_msg.as_bytes(), target).await {
                        Ok(sent) => {
                            bytes_sent += sent;
                            debug!("Sent connection status packet: {} bytes", sent);
                        }
                        Err(e) => warn!("Failed to send connection status packet: {}", e),
                    }
                }
            }
        }

        if bytes_sent > 0 {
            debug!("Flushed binary packets: {} bytes total", bytes_sent);
        }
    }
}

/// Compatibility wrapper for existing code
pub struct BinaryUdpSenderCompat {
    binary_sender: Arc<BinaryUdpSender>,
}

impl BinaryUdpSenderCompat {
    pub fn from_binary(binary_sender: Arc<BinaryUdpSender>) -> Self {
        BinaryUdpSenderCompat { binary_sender }
    }

    pub async fn send_trade(&self, trade: &TradeData) -> Result<()> {
        self.binary_sender.send_trade(trade.clone())
    }

    pub async fn send_orderbook(&self, orderbook: &OrderBookData) -> Result<()> {
        self.binary_sender.send_orderbook(orderbook.clone())
    }

    pub async fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.binary_sender.send_stats(exchange, trades, orderbooks)
    }

    pub fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize, reconnect_count: usize, total: usize) -> Result<()> {
        self.binary_sender.send_connection_status(exchange, connected, disconnected, reconnect_count, total)
    }
}

// Global binary UDP sender instance
pub static GLOBAL_BINARY_UDP_SENDER: OnceCell<Arc<BinaryUdpSender>> = OnceCell::new();

pub async fn init_global_binary_udp_sender(bind_addr: &str, target_addr: &str) -> Result<()> {
    let sender = BinaryUdpSender::new(bind_addr, target_addr).await?;
    GLOBAL_BINARY_UDP_SENDER.set(Arc::new(sender))
        .map_err(|_| crate::error::Error::Config("Failed to initialize binary UDP sender".to_string()))?;
    info!("Initialized global binary UDP sender: {} -> {}", bind_addr, target_addr);
    Ok(())
}

pub fn get_binary_udp_sender() -> Option<Arc<BinaryUdpSender>> {
    GLOBAL_BINARY_UDP_SENDER.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{TradeData, OrderBookData};

    #[test]
    fn test_binary_packet_sizes() {
        // Trade packet: 66 bytes header + 32 bytes trade item = 98 bytes
        let trade = TradeData {
            exchange: "Test".to_string(),
            symbol: "BTC-USD".to_string(),
            asset_type: "spot".to_string(),
            price: 50000.0,
            quantity: 1.0,
            timestamp: 1234567890,
        };

        let binary_packet = crate::core::BinaryUdpPacket::from_trade(&trade, 1);
        assert_eq!(binary_packet.size(), 66 + 32); // 98 bytes total

        // OrderBook packet: 66 bytes header + (N * 16 bytes per item)
        let orderbook = OrderBookData::new(
            "Test".to_string(),
            "BTC-USD".to_string(),
            "spot".to_string(),
            vec![(50000.0, 1.0), (49999.0, 2.0)], // 2 bids
            vec![(50001.0, 1.5), (50002.0, 2.5)], // 2 asks
            1234567890,
        );

        let binary_packet = crate::core::BinaryUdpPacket::from_orderbook(&orderbook, 1, true);
        assert_eq!(binary_packet.size(), 66 + (2 * 16)); // 66 + 32 = 98 bytes for bids
    }
}