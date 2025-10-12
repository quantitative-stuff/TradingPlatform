/// Multi-Port UDP Sender with Proportional Address Allocation
///
/// **Proportional Port Allocation (20 Total Addresses):**
/// - Binance: 8 addresses (40%)
/// - Bybit: 2 addresses (10%)
/// - OKX: 2 addresses (10%)
/// - Deribit: 2 addresses (10%)
/// - Upbit: 2 addresses (10%)
/// - Coinbase: 2 addresses (10%)
/// - Bithumb: 2 addresses (10%)
///
/// **Multicast Address Schema:**
/// Each exchange gets dedicated addresses for Trade and OrderBook data
///
/// **Binance (8 addresses):**
/// - Trades: 239.10.1.1:9000-9003 (4 addresses)
/// - OrderBooks: 239.10.1.1:9100-9103 (4 addresses)
///
/// **Other Exchanges (2 addresses each):**
/// - Bybit: 239.10.2.1:9000 (trade), 239.10.2.1:9100 (orderbook)
/// - OKX: 239.10.3.1:9000 (trade), 239.10.3.1:9100 (orderbook)
/// - Deribit: 239.10.4.1:9000 (trade), 239.10.4.1:9100 (orderbook)
/// - Upbit: 239.10.5.1:9000 (trade), 239.10.5.1:9100 (orderbook)
/// - Coinbase: 239.10.6.1:9000 (trade), 239.10.6.1:9100 (orderbook)
/// - Bithumb: 239.10.7.1:9000 (trade), 239.10.7.1:9100 (orderbook)

use crate::core::{BinaryUdpPacket, TradeData, OrderBookData};
use std::net::UdpSocket;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use anyhow::{Result, Context};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::{info, warn};
use socket2::{Socket, Domain, Type, Protocol};

/// Exchange configuration
#[derive(Debug, Clone, Copy)]
pub struct ExchangeConfig {
    pub id: u8,
    pub name: &'static str,
    pub address_count: usize,  // Total addresses (trade + orderbook)
}

const EXCHANGES: [ExchangeConfig; 7] = [
    ExchangeConfig { id: 1, name: "binance",  address_count: 8 },  // 4 trade + 4 orderbook
    ExchangeConfig { id: 2, name: "bybit",    address_count: 2 },  // 1 trade + 1 orderbook
    ExchangeConfig { id: 3, name: "okx",      address_count: 2 },
    ExchangeConfig { id: 4, name: "deribit",  address_count: 2 },
    ExchangeConfig { id: 5, name: "upbit",    address_count: 2 },
    ExchangeConfig { id: 6, name: "coinbase", address_count: 2 },
    ExchangeConfig { id: 7, name: "bithumb",  address_count: 2 },
];

/// Data type routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    Trade,
    OrderBook,
}

/// Address pool for one exchange + data type
struct AddressPool {
    exchange_name: String,
    exchange_id: u8,
    data_type: DataType,
    sockets: Vec<UdpSocket>,
    stats: Vec<u64>,
}

impl AddressPool {
    fn new(config: &ExchangeConfig, data_type: DataType) -> Result<Self> {
        let base_addr = format!("239.10.{}.1", config.id);

        // Calculate number of ports for this data type
        let port_count = if config.id == 1 { 4 } else { 1 }; // Binance gets 4, others get 1

        let base_port = match data_type {
            DataType::Trade => 9000,
            DataType::OrderBook => 9100,
        };

        let mut sockets = Vec::with_capacity(port_count);

        for i in 0..port_count {
            let port = base_port + i as u16;
            let target_addr = format!("{}:{}", base_addr, port);

            // Use socket2 for advanced socket options
            let socket2 = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
                .with_context(|| format!("Failed to create socket for {}", target_addr))?;

            socket2.set_send_buffer_size(2 * 1024 * 1024)?; // 2MB buffer
            socket2.set_multicast_ttl_v4(32)?; // Cross-machine capable
            socket2.set_multicast_loop_v4(false)?;

            // Bind to any port
            let bind_addr: std::net::SocketAddr = "0.0.0.0:0".parse().unwrap();
            socket2.bind(&bind_addr.into())?;

            // Convert to std::net::UdpSocket
            let socket: UdpSocket = socket2.into();

            socket.connect(&target_addr)
                .with_context(|| format!("Failed to connect to {}", target_addr))?;

            sockets.push(socket);
        }

        Ok(AddressPool {
            exchange_name: config.name.to_string(),
            exchange_id: config.id,
            data_type,
            sockets,
            stats: vec![0; port_count],
        })
    }

    fn select_socket(&self, symbol: &str) -> usize {
        if self.sockets.len() == 1 {
            0
        } else {
            let mut hasher = DefaultHasher::new();
            symbol.hash(&mut hasher);
            (hasher.finish() as usize) % self.sockets.len()
        }
    }

    fn send(&mut self, symbol: &str, data: &[u8]) -> Result<()> {
        let idx = self.select_socket(symbol);
        self.sockets[idx].send(data)?;
        self.stats[idx] += 1;
        Ok(())
    }

    fn get_stats(&self) -> (u64, Vec<u64>) {
        let total: u64 = self.stats.iter().sum();
        (total, self.stats.clone())
    }
}

/// Multi-port UDP sender with proportional allocation
pub struct MultiPortUdpSender {
    pools: HashMap<(String, DataType), Arc<RwLock<AddressPool>>>,
}

impl MultiPortUdpSender {
    pub fn new() -> Result<Self> {
        let mut pools = HashMap::new();

        info!("Initializing Multi-Port UDP Sender (Proportional)");
        info!("Address allocation (20 total):");
        info!("  Binance  → 8 addresses (4 trade + 4 orderbook)");
        info!("  Others   → 2 addresses each (1 trade + 1 orderbook)");

        for exchange in &EXCHANGES {
            let trade_pool = AddressPool::new(exchange, DataType::Trade)?;
            pools.insert(
                (exchange.name.to_string(), DataType::Trade),
                Arc::new(RwLock::new(trade_pool)),
            );

            let orderbook_pool = AddressPool::new(exchange, DataType::OrderBook)?;
            pools.insert(
                (exchange.name.to_string(), DataType::OrderBook),
                Arc::new(RwLock::new(orderbook_pool)),
            );
        }

        info!("✅ All 20 multicast addresses initialized");
        Ok(MultiPortUdpSender { pools })
    }

    pub fn send_trade(&self, exchange: &str, symbol: &str, packet_bytes: &[u8]) -> Result<()> {
        let key = (exchange.to_lowercase(), DataType::Trade);
        let pool = self.pools
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("Trade pool not found: {}", exchange))?;
        pool.write().send(symbol, packet_bytes)
    }

    pub fn send_orderbook(&self, exchange: &str, symbol: &str, packet_bytes: &[u8]) -> Result<()> {
        let key = (exchange.to_lowercase(), DataType::OrderBook);
        let pool = self.pools
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("OrderBook pool not found: {}", exchange))?;
        pool.write().send(symbol, packet_bytes)
    }

    pub fn send_packet(&self, mut packet: BinaryUdpPacket) -> Result<()> {
        let symbol = parse_name(&packet.header.symbol);
        let exchange = parse_name(&packet.header.exchange);
        let packet_type = packet.header.packet_type();
        let bytes = packet.to_bytes();

        match packet_type {
            1 => self.send_trade(&exchange, &symbol, &bytes),
            2 => self.send_orderbook(&exchange, &symbol, &bytes),
            _ => Err(anyhow::anyhow!("Unknown packet type: {}", packet_type)),
        }
    }

    pub fn get_total_packets(&self) -> u64 {
        self.pools.values().map(|p| p.read().get_stats().0).sum()
    }

    /// Helper: Send trade (accepts TradeData struct)
    pub fn send_trade_data(&self, trade: TradeData) -> Result<()> {
        let mut packet = BinaryUdpPacket::from_trade(&trade);
        self.send_packet(packet)
    }

    /// Helper: Send orderbook (accepts OrderBookData struct)
    pub fn send_orderbook_data(&self, orderbook: OrderBookData) -> Result<()> {
        // Send bids
        if !orderbook.bids.is_empty() {
            let bid_packet = BinaryUdpPacket::from_orderbook(&orderbook, true);
            self.send_packet(bid_packet)?;
        }

        // Send asks
        if !orderbook.asks.is_empty() {
            let ask_packet = BinaryUdpPacket::from_orderbook(&orderbook, false);
            self.send_packet(ask_packet)?;
        }

        Ok(())
    }

    pub fn print_stats(&self) {
        println!("\n╔══════════════════════════════════════════════════════╗");
        println!("║  Multi-Port UDP Sender Statistics (Proportional)    ║");
        println!("╠══════════════════════════════════════════════════════╣");
        println!("║  Total: 20 multicast addresses                       ║");
        println!("╚══════════════════════════════════════════════════════╝\n");

        let mut grand_total = 0u64;

        for exchange in &EXCHANGES {
            let trade_key = (exchange.name.to_string(), DataType::Trade);
            let orderbook_key = (exchange.name.to_string(), DataType::OrderBook);

            if let (Some(tp), Some(op)) = (self.pools.get(&trade_key), self.pools.get(&orderbook_key)) {
                let (tt, ts) = tp.read().get_stats();
                let (ot, os) = op.read().get_stats();
                let total = tt + ot;
                grand_total += total;

                if total > 0 {
                    println!("┌─ {} ({} addresses) ─────", exchange.name.to_uppercase(), exchange.address_count);
                    if tt > 0 {
                        println!("│  📈 Trades: {} packets", tt);
                        for (i, &c) in ts.iter().enumerate() {
                            if c > 0 {
                                println!("│    Addr {}: {} pkts", i, c);
                            }
                        }
                    }
                    if ot > 0 {
                        println!("│  📊 OrderBooks: {} packets", ot);
                        for (i, &c) in os.iter().enumerate() {
                            if c > 0 {
                                println!("│    Addr {}: {} pkts", i, c);
                            }
                        }
                    }
                    println!("│  Total: {}\n", total);
                }
            }
        }

        println!("╔══════════════════════════════════════════════════════╗");
        println!("║  GRAND TOTAL: {:>10} packets                      ║", grand_total);
        println!("╚══════════════════════════════════════════════════════╝\n");
    }
}

fn parse_name(bytes: &[u8]) -> String {
    let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..null_pos]).to_string()
}

// Global instance - Using OnceCell for lock-free reads after initialization
use once_cell::sync::OnceCell;

static GLOBAL_MULTI_PORT_SENDER: OnceCell<Arc<MultiPortUdpSender>> = OnceCell::new();

pub fn init_global_multi_port_sender() -> Result<()> {
    let sender = MultiPortUdpSender::new()?;
    GLOBAL_MULTI_PORT_SENDER.set(Arc::new(sender))
        .map_err(|_| anyhow::anyhow!("Multi-port sender already initialized"))?;
    info!("Global multi-port UDP sender initialized successfully");
    Ok(())
}

pub fn get_multi_port_sender() -> Option<Arc<MultiPortUdpSender>> {
    GLOBAL_MULTI_PORT_SENDER.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sender_creation() {
        let sender = MultiPortUdpSender::new();
        assert!(sender.is_ok());
        let sender = sender.unwrap();
        assert_eq!(sender.pools.len(), 14); // 7 exchanges × 2 data types
    }
}
