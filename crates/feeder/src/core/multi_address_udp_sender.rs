use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use crate::core::{TradeData, OrderBookData};
use crate::error::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use once_cell::sync::OnceCell;
use tracing::{info, debug, warn, error};

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

/// Configuration for multi-address UDP sending
#[derive(Debug, Clone)]
pub struct MultiAddressConfig {
    /// List of target addresses to send packets to
    pub target_addresses: Vec<String>,
    /// Routing strategy: "broadcast" (send to all), "round-robin", "exchange-based"
    pub routing_strategy: String,
    /// Optional: Map exchanges to specific addresses (for exchange-based routing)
    pub exchange_routing: Option<std::collections::HashMap<String, Vec<String>>>,
}

impl Default for MultiAddressConfig {
    fn default() -> Self {
        Self {
            target_addresses: vec!["239.255.42.99:9001".to_string()],
            routing_strategy: "broadcast".to_string(),
            exchange_routing: None,
        }
    }
}

/// High-performance multi-address UDP sender supporting multiple targets
pub struct MultiAddressUdpSender {
    tx: mpsc::UnboundedSender<BinaryUdpMessage>,
    config: Arc<MultiAddressConfig>,
}

impl MultiAddressUdpSender {
    /// Create new multi-address UDP sender
    pub async fn new(bind_addr: &str, config: MultiAddressConfig) -> Result<Self> {
        // Validate configuration
        if config.target_addresses.is_empty() {
            return Err(crate::error::Error::Config(
                "No target addresses configured".to_string()
            ));
        }

        info!("Initializing multi-address UDP sender with {} targets",
              config.target_addresses.len());
        for addr in &config.target_addresses {
            info!("  Target: {}", addr);
        }
        info!("  Routing strategy: {}", config.routing_strategy);

        let socket = UdpSocket::bind(bind_addr).await?;

        // Configure socket for broadcast/multicast
        socket.set_broadcast(true)?;

        // Check if any addresses are multicast and configure accordingly
        for target_addr in &config.target_addresses {
            if target_addr.starts_with("239.") || target_addr.starts_with("224.") {
                socket.set_multicast_ttl_v4(1)?; // Same subnet
                socket.set_multicast_loop_v4(false)?; // Don't receive own packets
                info!("Configured for multicast to {}", target_addr);
                break; // Only need to set once
            }
        }

        let socket = Arc::new(socket);
        let config = Arc::new(config);
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Spawn the packet processing task
        let socket_clone = socket.clone();
        let config_clone = config.clone();

        tokio::spawn(async move {
            let mut packet_buffer: Vec<BinaryUdpMessage> = Vec::with_capacity(BATCH_SIZE);
            let mut last_flush = Instant::now();
            let mut round_robin_index = 0usize;

            info!("Multi-address UDP sender task started");

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

                            Self::flush_packets_multi(
                                &socket_clone,
                                &config_clone,
                                &mut packet_buffer,
                                &mut round_robin_index
                            ).await;
                            last_flush = Instant::now();
                        }
                    }
                    Ok(None) => {
                        // Channel closed
                        info!("Multi-address UDP sender channel closed, exiting");
                        break;
                    }
                    Err(_) => {
                        // Timeout - flush any pending packets
                        if !packet_buffer.is_empty() {
                            Self::flush_packets_multi(
                                &socket_clone,
                                &config_clone,
                                &mut packet_buffer,
                                &mut round_robin_index
                            ).await;
                            last_flush = Instant::now();
                        }
                    }
                }
            }
        });

        Ok(MultiAddressUdpSender { tx, config })
    }

    /// Send trade data
    pub fn send_trade(&self, trade: TradeData) -> Result<()> {
        self.tx.send(BinaryUdpMessage::Trade(trade))
            .map_err(|_| crate::error::Error::Connection("Multi UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Send orderbook data
    pub fn send_orderbook(&self, orderbook: OrderBookData) -> Result<()> {
        self.tx.send(BinaryUdpMessage::OrderBook(orderbook))
            .map_err(|_| crate::error::Error::Connection("Multi UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Send stats
    pub fn send_stats(&self, exchange: &str, trades: usize, orderbooks: usize) -> Result<()> {
        self.tx.send(BinaryUdpMessage::Stats {
            exchange: exchange.to_string(),
            trades,
            orderbooks,
        }).map_err(|_| crate::error::Error::Connection("Multi UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Send connection status
    pub fn send_connection_status(&self, exchange: &str, connected: usize, disconnected: usize,
                                  reconnect_count: usize, total: usize) -> Result<()> {
        self.tx.send(BinaryUdpMessage::ConnectionStatus {
            exchange: exchange.to_string(),
            connected,
            disconnected,
            reconnect_count,
            total,
        }).map_err(|_| crate::error::Error::Connection("Multi UDP channel closed".to_string()))?;
        Ok(())
    }

    /// Flush packets to multiple UDP addresses based on routing strategy
    async fn flush_packets_multi(
        socket: &UdpSocket,
        config: &Arc<MultiAddressConfig>,
        packets: &mut Vec<BinaryUdpMessage>,
        round_robin_index: &mut usize,
    ) {
        for packet in packets.drain(..) {
            let data = Self::serialize_packet(&packet).await;
            if data.is_empty() {
                continue;
            }

            // Determine target addresses based on routing strategy
            let targets = match config.routing_strategy.as_str() {
                "broadcast" => {
                    // Send to all configured addresses
                    config.target_addresses.clone()
                }
                "round-robin" => {
                    // Send to next address in rotation
                    let target = config.target_addresses[*round_robin_index % config.target_addresses.len()].clone();
                    *round_robin_index += 1;
                    vec![target]
                }
                "exchange-based" => {
                    // Route based on exchange (if configured)
                    let exchange = Self::get_exchange_from_packet(&packet);
                    if let Some(routing) = &config.exchange_routing {
                        if let Some(addrs) = routing.get(&exchange) {
                            addrs.clone()
                        } else {
                            // Fallback to all addresses if exchange not in routing map
                            config.target_addresses.clone()
                        }
                    } else {
                        config.target_addresses.clone()
                    }
                }
                _ => {
                    warn!("Unknown routing strategy: {}, defaulting to broadcast",
                          config.routing_strategy);
                    config.target_addresses.clone()
                }
            };

            // Send packet to all determined target addresses
            for target in &targets {
                match socket.send_to(&data, target).await {
                    Ok(bytes) => {
                        debug!("Sent {} bytes to {}", bytes, target);
                    }
                    Err(e) => {
                        error!("Failed to send to {}: {}", target, e);
                    }
                }
            }
        }
    }

    /// Serialize packet to binary format
    async fn serialize_packet(packet: &BinaryUdpMessage) -> Vec<u8> {
        match packet {
            BinaryUdpMessage::Trade(trade) => {
                // Create binary trade packet (similar to existing implementation)
                let exchange_bytes = trade.exchange.as_bytes();
                let symbol_bytes = trade.symbol.as_bytes();

                if exchange_bytes.len() > 10 || symbol_bytes.len() > 20 {
                    warn!("Trade data too long: exchange={} symbol={}",
                          trade.exchange, trade.symbol);
                    return Vec::new();
                }

                let mut packet = vec![0u8; 66];
                packet[0] = 0x01; // Trade packet type

                // Pack exchange (10 bytes)
                packet[1..1+exchange_bytes.len()].copy_from_slice(exchange_bytes);

                // Pack symbol (20 bytes)
                packet[11..11+symbol_bytes.len()].copy_from_slice(symbol_bytes);

                // Pack price and quantity as scaled integers
                let price_scaled = (trade.price * 1e8) as u64;
                let qty_scaled = (trade.quantity * 1e8) as u64;

                packet[31..39].copy_from_slice(&price_scaled.to_le_bytes());
                packet[39..47].copy_from_slice(&qty_scaled.to_le_bytes());

                // Pack timestamp
                packet[47..55].copy_from_slice(&trade.timestamp.to_le_bytes());

                // Pack side
                packet[55] = if trade.side == "buy" { 1 } else { 0 };

                packet
            }
            BinaryUdpMessage::OrderBook(orderbook) => {
                // Serialize orderbook (implementation similar to existing)
                // For brevity, using text format here
                let json = serde_json::to_string(orderbook).unwrap_or_default();
                format!("ORDERBOOK|{}", json).into_bytes()
            }
            BinaryUdpMessage::Stats { exchange, trades, orderbooks } => {
                // Text format for stats
                format!("STATS|{}|{}|{}|{}",
                    exchange, trades, orderbooks,
                    chrono::Local::now().timestamp()
                ).into_bytes()
            }
            BinaryUdpMessage::ConnectionStatus { exchange, connected, disconnected, reconnect_count, total } => {
                // Text format for connection status
                format!("CONN|{}|{}|{}|{}|{}",
                    exchange, connected, disconnected, reconnect_count, total
                ).into_bytes()
            }
        }
    }

    /// Extract exchange name from packet for routing
    fn get_exchange_from_packet(packet: &BinaryUdpMessage) -> String {
        match packet {
            BinaryUdpMessage::Trade(t) => t.exchange.clone(),
            BinaryUdpMessage::OrderBook(o) => o.exchange.clone(),
            BinaryUdpMessage::Stats { exchange, .. } => exchange.clone(),
            BinaryUdpMessage::ConnectionStatus { exchange, .. } => exchange.clone(),
        }
    }

    /// Get current configuration
    pub fn get_config(&self) -> Arc<MultiAddressConfig> {
        self.config.clone()
    }

    /// Update target addresses dynamically
    pub async fn update_targets(&mut self, new_targets: Vec<String>) -> Result<()> {
        if new_targets.is_empty() {
            return Err(crate::error::Error::Config(
                "Cannot update to empty target list".to_string()
            ));
        }

        let mut config = MultiAddressConfig::clone(&self.config);
        config.target_addresses = new_targets;
        self.config = Arc::new(config);

        info!("Updated target addresses to: {:?}", self.config.target_addresses);
        Ok(())
    }
}

// Global multi-address UDP sender instance
pub static GLOBAL_MULTI_UDP_SENDER: OnceCell<Arc<MultiAddressUdpSender>> = OnceCell::new();

/// Initialize global multi-address UDP sender with default config
pub async fn init_global_multi_udp_sender(bind_addr: &str, targets: Vec<String>) -> Result<()> {
    let config = MultiAddressConfig {
        target_addresses: targets,
        routing_strategy: "broadcast".to_string(),
        exchange_routing: None,
    };

    let sender = MultiAddressUdpSender::new(bind_addr, config).await?;
    GLOBAL_MULTI_UDP_SENDER.set(Arc::new(sender))
        .map_err(|_| crate::error::Error::Config("Failed to initialize global multi-UDP sender".to_string()))?;
    Ok(())
}

/// Initialize with full configuration
pub async fn init_global_multi_udp_sender_with_config(bind_addr: &str, config: MultiAddressConfig) -> Result<()> {
    let sender = MultiAddressUdpSender::new(bind_addr, config).await?;
    GLOBAL_MULTI_UDP_SENDER.set(Arc::new(sender))
        .map_err(|_| crate::error::Error::Config("Failed to initialize global multi-UDP sender".to_string()))?;
    Ok(())
}

/// Get the global multi-address UDP sender
pub fn get_multi_udp_sender() -> Option<Arc<MultiAddressUdpSender>> {
    GLOBAL_MULTI_UDP_SENDER.get().cloned()
}