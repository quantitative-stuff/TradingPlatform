/// Multi-port UDP sender for distributing load across NIC queues
/// Partitions symbols across multiple multicast ports for better RSS distribution
/// Based on architecture: docs/feeder/UDP_Multicast_architecture.md

use crate::core::BinaryUdpPacket;
use std::net::UdpSocket;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use parking_lot::RwLock;
use anyhow::Result;

/// Multi-port configuration
#[derive(Debug, Clone)]
pub struct MultiPortConfig {
    /// Base multicast address (e.g., "239.0.0.1")
    pub base_address: String,

    /// Starting port number
    pub base_port: u16,

    /// Number of ports to use (typically 4-8)
    pub num_ports: usize,

    /// Partitioning strategy
    pub strategy: PartitionStrategy,
}

impl Default for MultiPortConfig {
    fn default() -> Self {
        MultiPortConfig {
            base_address: "239.0.0.1".to_string(),
            base_port: 9001,
            num_ports: 4,
            strategy: PartitionStrategy::SymbolHash,
        }
    }
}

/// How to partition symbols across ports
#[derive(Debug, Clone, Copy)]
pub enum PartitionStrategy {
    /// Hash symbol name (good for even distribution)
    SymbolHash,

    /// Hash exchange + symbol (keeps same symbol on different exchanges separate)
    ExchangeSymbolHash,

    /// Round-robin (simple, predictable)
    RoundRobin,
}

/// Multi-port UDP sender
pub struct MultiPortUdpSender {
    config: MultiPortConfig,
    sockets: Vec<UdpSocket>,
    stats: Arc<RwLock<MultiPortStats>>,
    round_robin_counter: Arc<RwLock<usize>>,
}

/// Statistics per port
#[derive(Debug, Clone)]
pub struct MultiPortStats {
    pub packets_per_port: Vec<u64>,
    pub bytes_per_port: Vec<u64>,
    pub errors_per_port: Vec<u64>,
}

impl MultiPortUdpSender {
    /// Create new multi-port sender
    pub fn new(config: MultiPortConfig) -> Result<Self> {
        let mut sockets = Vec::with_capacity(config.num_ports);

        // Create socket for each port
        for i in 0..config.num_ports {
            let port = config.base_port + i as u16;
            let addr = format!("{}:{}", config.base_address, port);

            let socket = UdpSocket::bind("0.0.0.0:0")?;
            socket.connect(&addr)?;

            // Optional: Set socket options using socket2 if needed
            // For now, use defaults

            sockets.push(socket);
        }

        let stats = MultiPortStats {
            packets_per_port: vec![0; config.num_ports],
            bytes_per_port: vec![0; config.num_ports],
            errors_per_port: vec![0; config.num_ports],
        };

        Ok(MultiPortUdpSender {
            config,
            sockets,
            stats: Arc::new(RwLock::new(stats)),
            round_robin_counter: Arc::new(RwLock::new(0)),
        })
    }

    /// Select port based on symbol and exchange
    fn select_port(&self, symbol: &str, exchange: &str) -> usize {
        match self.config.strategy {
            PartitionStrategy::SymbolHash => {
                let mut hasher = DefaultHasher::new();
                symbol.hash(&mut hasher);
                (hasher.finish() as usize) % self.config.num_ports
            }
            PartitionStrategy::ExchangeSymbolHash => {
                let mut hasher = DefaultHasher::new();
                exchange.hash(&mut hasher);
                symbol.hash(&mut hasher);
                (hasher.finish() as usize) % self.config.num_ports
            }
            PartitionStrategy::RoundRobin => {
                let mut counter = self.round_robin_counter.write();
                let port = *counter % self.config.num_ports;
                *counter = counter.wrapping_add(1);
                port
            }
        }
    }

    /// Send binary packet to appropriate port
    pub fn send_packet(&self, mut packet: BinaryUdpPacket) -> Result<()> {
        // Extract symbol and exchange from header
        let symbol = parse_name(&packet.header.symbol);
        let exchange = parse_name(&packet.header.exchange);

        // Select port
        let port_idx = self.select_port(&symbol, &exchange);

        // Serialize packet
        let bytes = packet.to_bytes();
        let size = bytes.len();

        // Send to selected port
        match self.sockets[port_idx].send(&bytes) {
            Ok(_) => {
                let mut stats = self.stats.write();
                stats.packets_per_port[port_idx] += 1;
                stats.bytes_per_port[port_idx] += size as u64;
                Ok(())
            }
            Err(e) => {
                let mut stats = self.stats.write();
                stats.errors_per_port[port_idx] += 1;
                Err(e.into())
            }
        }
    }

    /// Send orderbook update (helper)
    pub fn send_orderbook(
        &self,
        exchange: &str,
        symbol: &str,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
        exchange_ts: u64,
    ) -> Result<()> {
        // Send bids
        if !bids.is_empty() {
            let bid_packet = BinaryUdpPacket::from_orderbook(
                &crate::core::OrderBookData {
                    exchange: exchange.to_string(),
                    symbol: symbol.to_string(),
                    bids: bids.to_vec(),
                    asks: vec![],
                    timestamp: exchange_ts as i64 / 1_000_000, // ns to ms
                    asset_type: "crypto".to_string(),
                },
                true, // is_bid
            );
            self.send_packet(bid_packet)?;
        }

        // Send asks
        if !asks.is_empty() {
            let ask_packet = BinaryUdpPacket::from_orderbook(
                &crate::core::OrderBookData {
                    exchange: exchange.to_string(),
                    symbol: symbol.to_string(),
                    bids: vec![],
                    asks: asks.to_vec(),
                    timestamp: exchange_ts as i64 / 1_000_000, // ns to ms
                    asset_type: "crypto".to_string(),
                },
                false, // is_ask
            );
            self.send_packet(ask_packet)?;
        }

        Ok(())
    }

    /// Send trade update (helper)
    pub fn send_trade(
        &self,
        exchange: &str,
        symbol: &str,
        price: f64,
        quantity: f64,
        is_buy: bool,
        exchange_ts: u64,
    ) -> Result<()> {
        let trade_packet = BinaryUdpPacket::from_trade(&crate::core::TradeData {
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            price,
            quantity,
            timestamp: exchange_ts as i64 / 1_000_000, // ns to ms
            asset_type: "crypto".to_string(),
        });

        self.send_packet(trade_packet)
    }

    /// Get current statistics
    pub fn get_stats(&self) -> MultiPortStats {
        self.stats.read().clone()
    }

    /// Print statistics summary
    pub fn print_stats(&self) {
        let stats = self.stats.read();
        println!("=== Multi-Port UDP Sender Statistics ===");
        println!("Strategy: {:?}", self.config.strategy);
        println!("Ports: {} - {}", self.config.base_port, self.config.base_port + self.config.num_ports as u16 - 1);
        println!();

        let total_packets: u64 = stats.packets_per_port.iter().sum();
        let total_bytes: u64 = stats.bytes_per_port.iter().sum();
        let total_errors: u64 = stats.errors_per_port.iter().sum();

        for i in 0..self.config.num_ports {
            let port = self.config.base_port + i as u16;
            let packets = stats.packets_per_port[i];
            let bytes = stats.bytes_per_port[i];
            let errors = stats.errors_per_port[i];
            let pct = if total_packets > 0 {
                (packets as f64 / total_packets as f64) * 100.0
            } else {
                0.0
            };

            println!(
                "Port {}: {:>8} packets ({:>5.1}%), {:>10} bytes, {:>4} errors",
                port, packets, pct, bytes, errors
            );
        }

        println!();
        println!("Total: {} packets, {} bytes, {} errors", total_packets, total_bytes, total_errors);
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        stats.packets_per_port.iter_mut().for_each(|v| *v = 0);
        stats.bytes_per_port.iter_mut().for_each(|v| *v = 0);
        stats.errors_per_port.iter_mut().for_each(|v| *v = 0);
    }
}

/// Parse null-terminated string from fixed-size array
fn parse_name(bytes: &[u8]) -> String {
    let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..null_pos]).to_string()
}

// Global instance (optional)
use once_cell::sync::Lazy;

static GLOBAL_MULTI_PORT_SENDER: Lazy<RwLock<Option<Arc<MultiPortUdpSender>>>> =
    Lazy::new(|| RwLock::new(None));

/// Initialize global multi-port sender
pub fn init_global_multi_port_sender(config: MultiPortConfig) -> Result<()> {
    let sender = MultiPortUdpSender::new(config)?;
    *GLOBAL_MULTI_PORT_SENDER.write() = Some(Arc::new(sender));
    Ok(())
}

/// Get global multi-port sender
pub fn get_multi_port_sender() -> Option<Arc<MultiPortUdpSender>> {
    GLOBAL_MULTI_PORT_SENDER.read().as_ref().map(Arc::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_selection_symbol_hash() {
        let config = MultiPortConfig {
            base_address: "239.0.0.1".to_string(),
            base_port: 9001,
            num_ports: 4,
            strategy: PartitionStrategy::SymbolHash,
        };

        let sender = MultiPortUdpSender::new(config).unwrap();

        // Same symbol should always go to same port
        let port1 = sender.select_port("BTC/USDT", "binance");
        let port2 = sender.select_port("BTC/USDT", "binance");
        assert_eq!(port1, port2);

        // Same symbol on different exchange should go to same port (SymbolHash)
        let port3 = sender.select_port("BTC/USDT", "bybit");
        assert_eq!(port1, port3);

        // Different symbol should (likely) go to different port
        let port4 = sender.select_port("ETH/USDT", "binance");
        // Not guaranteed but very likely
        println!("BTC port: {}, ETH port: {}", port1, port4);
    }

    #[test]
    fn test_port_selection_exchange_symbol_hash() {
        let config = MultiPortConfig {
            base_address: "239.0.0.1".to_string(),
            base_port: 9001,
            num_ports: 4,
            strategy: PartitionStrategy::ExchangeSymbolHash,
        };

        let sender = MultiPortUdpSender::new(config).unwrap();

        // Same symbol on different exchange may go to different port
        let port1 = sender.select_port("BTC/USDT", "binance");
        let port2 = sender.select_port("BTC/USDT", "bybit");
        println!("Binance BTC port: {}, Bybit BTC port: {}", port1, port2);
    }

    #[test]
    fn test_port_selection_round_robin() {
        let config = MultiPortConfig {
            base_address: "239.0.0.1".to_string(),
            base_port: 9001,
            num_ports: 4,
            strategy: PartitionStrategy::RoundRobin,
        };

        let sender = MultiPortUdpSender::new(config).unwrap();

        // Should cycle through ports
        let ports: Vec<usize> = (0..8)
            .map(|_| sender.select_port("BTC/USDT", "binance"))
            .collect();

        assert_eq!(ports, vec![0, 1, 2, 3, 0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_name() {
        let bytes = b"binance\0\0\0\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(parse_name(bytes), "binance");

        let bytes = b"BTC/USDT\0\0\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(parse_name(bytes), "BTC/USDT");
    }
}