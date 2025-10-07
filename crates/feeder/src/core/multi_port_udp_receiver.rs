/// Multi-Threaded Proportional UDP Receiver
///
/// Receives data from 20 multicast addresses in parallel:
/// - Binance: 8 receiver threads (4 trade + 4 orderbook)
/// - Others: 2 receiver threads each (1 trade + 1 orderbook)
///
/// Each receiver runs in its own thread for maximum throughput

use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use std::net::Ipv4Addr;
use anyhow::{Result, Context};
use tracing::{info, error};
use std::sync::Arc;
use parking_lot::RwLock;
use std::collections::HashMap;
use socket2::{Socket, Domain, Type, Protocol};

/// Exchange configuration matching sender
#[derive(Debug, Clone, Copy)]
struct ExchangeConfig {
    id: u8,
    name: &'static str,
    address_count: usize,
}

const EXCHANGES: [ExchangeConfig; 7] = [
    ExchangeConfig { id: 1, name: "binance",  address_count: 8 },
    ExchangeConfig { id: 2, name: "bybit",    address_count: 2 },
    ExchangeConfig { id: 3, name: "okx",      address_count: 2 },
    ExchangeConfig { id: 4, name: "deribit",  address_count: 2 },
    ExchangeConfig { id: 5, name: "upbit",    address_count: 2 },
    ExchangeConfig { id: 6, name: "coinbase", address_count: 2 },
    ExchangeConfig { id: 7, name: "bithumb",  address_count: 2 },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    Trade,
    OrderBook,
}

/// Statistics per receiver thread
#[derive(Debug, Clone, Default)]
pub struct ReceiverStats {
    pub packets_received: u64,
    pub bytes_received: u64,
    pub errors: u64,
}

/// Multi-threaded multi-port receiver
pub struct MultiPortUdpReceiver {
    handles: Vec<JoinHandle<()>>,
    stats: Arc<RwLock<HashMap<String, ReceiverStats>>>,
}

impl MultiPortUdpReceiver {
    /// Create and start all receiver threads
    pub async fn new<F, Fut>(handler: F) -> Result<Self>
    where
        F: Fn(Vec<u8>, String, String, DataType) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut handles = Vec::new();
        let stats = Arc::new(RwLock::new(HashMap::new()));

        info!("Starting Multi-Port UDP Receiver (20 addresses)");

        // Start receivers for all exchanges
        for exchange in &EXCHANGES {
            let port_count = if exchange.id == 1 { 4 } else { 1 };

            // Trade receivers
            for i in 0..port_count {
                let handle = Self::start_receiver(
                    exchange,
                    DataType::Trade,
                    i,
                    handler.clone(),
                    stats.clone(),
                ).await?;
                handles.push(handle);
            }

            // OrderBook receivers
            for i in 0..port_count {
                let handle = Self::start_receiver(
                    exchange,
                    DataType::OrderBook,
                    i,
                    handler.clone(),
                    stats.clone(),
                ).await?;
                handles.push(handle);
            }
        }

        info!("✅ Started {} receiver threads", handles.len());

        Ok(MultiPortUdpReceiver { handles, stats })
    }

    /// Start a single receiver thread
    async fn start_receiver<F, Fut>(
        exchange: &ExchangeConfig,
        data_type: DataType,
        port_offset: usize,
        handler: F,
        stats: Arc<RwLock<HashMap<String, ReceiverStats>>>,
    ) -> Result<JoinHandle<()>>
    where
        F: Fn(Vec<u8>, String, String, DataType) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let multicast_addr = format!("239.10.{}.1", exchange.id);
        let base_port = match data_type {
            DataType::Trade => 9000,
            DataType::OrderBook => 9100,
        };
        let port = base_port + port_offset as u16;

        // Use socket2 for buffer size control
        let socket2 = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .with_context(|| format!("Failed to create socket for port {}", port))?;

        socket2.set_recv_buffer_size(4 * 1024 * 1024)?; // 4MB buffer
        socket2.set_reuse_address(true)?;

        let bind_addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
        socket2.bind(&bind_addr.into())?;

        // Convert to tokio UdpSocket
        socket2.set_nonblocking(true)?;
        let std_socket: std::net::UdpSocket = socket2.into();
        let socket = UdpSocket::from_std(std_socket)?;

        let multicast_ip: Ipv4Addr = multicast_addr.parse()
            .with_context(|| format!("Invalid multicast address: {}", multicast_addr))?;

        socket.join_multicast_v4(multicast_ip, Ipv4Addr::UNSPECIFIED)
            .with_context(|| format!("Failed to join multicast {}", multicast_addr))?;

        socket.set_multicast_loop_v4(false)?;

        let receiver_id = format!(
            "{}_{}_{}",
            exchange.name,
            match data_type {
                DataType::Trade => "trade",
                DataType::OrderBook => "orderbook",
            },
            port_offset
        );

        info!(
            "✅ Receiver {} listening on {}:{}",
            receiver_id, multicast_addr, port
        );

        // Initialize stats
        stats.write().insert(receiver_id.clone(), ReceiverStats::default());

        let exchange_name = exchange.name.to_string();

        // Spawn receiver task
        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];

            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, _addr)) => {
                        let data = buf[..len].to_vec();

                        // Update stats
                        {
                            let mut stats_map = stats.write();
                            if let Some(stat) = stats_map.get_mut(&receiver_id) {
                                stat.packets_received += 1;
                                stat.bytes_received += len as u64;
                            }
                        }

                        // Process packet
                        handler(
                            data,
                            exchange_name.clone(),
                            receiver_id.clone(),
                            data_type,
                        ).await;
                    }
                    Err(e) => {
                        error!("Receiver {} error: {}", receiver_id, e);
                        {
                            let mut stats_map = stats.write();
                            if let Some(stat) = stats_map.get_mut(&receiver_id) {
                                stat.errors += 1;
                            }
                        } // Drop lock before await
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Get current statistics
    pub fn get_stats(&self) -> HashMap<String, ReceiverStats> {
        self.stats.read().clone()
    }

    /// Print statistics
    pub fn print_stats(&self) {
        info!("\n╔══════════════════════════════════════════════════════╗");
        info!("║  Multi-Port UDP Receiver Statistics                 ║");
        info!("╠══════════════════════════════════════════════════════╣");
        info!("║  Active Receivers: {}                                ║", self.handles.len());
        info!("╚══════════════════════════════════════════════════════╝\n");

        let stats = self.stats.read();
        let mut total_packets = 0u64;
        let mut total_bytes = 0u64;
        let mut total_errors = 0u64;

        for exchange in &EXCHANGES {
            let mut exchange_packets = 0u64;
            let mut exchange_bytes = 0u64;

            info!("┌─ {} ─────", exchange.name.to_uppercase());

            for data_type_name in &["trade", "orderbook"] {
                let port_count = if exchange.id == 1 { 4 } else { 1 };

                for i in 0..port_count {
                    let key = format!("{}_{}_{}", exchange.name, data_type_name, i);
                    if let Some(stat) = stats.get(&key) {
                        if stat.packets_received > 0 {
                            info!(
                                "│  {}: {} pkts, {} bytes",
                                key,
                                stat.packets_received,
                                stat.bytes_received
                            );
                            exchange_packets += stat.packets_received;
                            exchange_bytes += stat.bytes_received;
                            total_errors += stat.errors;
                        }
                    }
                }
            }

            if exchange_packets > 0 {
                info!("│  Total: {} packets, {} bytes\n", exchange_packets, exchange_bytes);
            } else {
                info!("│  No data received\n");
            }

            total_packets += exchange_packets;
            total_bytes += exchange_bytes;
        }

        info!("╔══════════════════════════════════════════════════════╗");
        info!("║  TOTAL: {} packets, {} MB               ║",
            total_packets,
            total_bytes / 1_000_000
        );
        if total_errors > 0 {
            info!("║  Errors: {}                                        ║", total_errors);
        }
        info!("╚══════════════════════════════════════════════════════╝\n");
    }

    /// Wait for all receivers to complete (runs forever unless aborted)
    pub async fn wait(self) {
        for handle in self.handles {
            let _ = handle.await;
        }
    }

    /// Abort all receiver threads
    pub fn abort_all(&self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

// Example handler for testing
#[allow(dead_code)]
async fn example_handler(
    data: Vec<u8>,
    exchange: String,
    receiver_id: String,
    data_type: DataType,
) {
    info!(
        "Received {} bytes from {} ({:?}) via {}",
        data.len(),
        exchange,
        data_type,
        receiver_id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_receiver_creation() {
        let receiver = MultiPortUdpReceiver::new(|data, ex, id, dt| async move {
            println!("Got {} bytes from {} ({:?}) via {}", data.len(), ex, dt, id);
        }).await;

        assert!(receiver.is_ok());
        let receiver = receiver.unwrap();
        assert_eq!(receiver.handles.len(), 20); // 8 for binance + 12 for others
    }
}
