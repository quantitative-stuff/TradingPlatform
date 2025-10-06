use market_types::{Exchange, OrderBookUpdate, Trade, Timestamp, Side, PriceLevel};
use anyhow::{Result, Context};
use bytes::BytesMut;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, broadcast};
use tokio::task::JoinHandle;
use tracing::{info, warn, error};

/// UDP packet protocol definition
#[repr(C, packed)]
struct UdpPacketHeader {
    magic: u32,           // Magic number for protocol validation
    sequence: u64,        // Sequence number for packet loss detection
    timestamp: i64,       // Microseconds since epoch
    exchange_id: u8,      // Exchange identifier
    message_type: u8,     // 0: OrderBook, 1: Trade
    symbol_len: u16,      // Symbol string length
}

pub struct UdpReceiver {
    exchanges: Vec<(Exchange, SocketAddr)>,
    orderbook_tx: mpsc::Sender<OrderBookUpdate>,
    trades_tx: mpsc::Sender<Trade>,
    handles: Vec<JoinHandle<()>>,
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl UdpReceiver {
    pub fn new(
        exchanges: Vec<(Exchange, u16)>, // Exchange and port pairs
        buffer_size: usize,
    ) -> (Self, mpsc::Receiver<OrderBookUpdate>, mpsc::Receiver<Trade>) {
        let (orderbook_tx, orderbook_rx) = mpsc::channel(buffer_size);
        let (trades_tx, trades_rx) = mpsc::channel(buffer_size);

        let exchange_addrs = exchanges
            .into_iter()
            .map(|(ex, port)| (ex, format!("0.0.0.0:{}", port).parse().unwrap()))
            .collect();

        (
            Self {
                exchanges: exchange_addrs,
                orderbook_tx,
                trades_tx,
                handles: Vec::new(),
                shutdown_tx: None,
            },
            orderbook_rx,
            trades_rx,
        )
    }

    pub async fn start(&mut self) -> Result<()> {
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        for (exchange, addr) in &self.exchanges {
            let socket = UdpSocket::bind(addr).await
                .with_context(|| format!("Failed to bind UDP socket for {}", exchange))?;

            info!("UDP receiver started for {} on {}", exchange, addr);

            let orderbook_tx = self.orderbook_tx.clone();
            let trades_tx = self.trades_tx.clone();
            let exchange = *exchange;
            let mut shutdown_rx = shutdown_tx.subscribe();

            let handle = tokio::spawn(async move {
                let mut buf = BytesMut::with_capacity(65536);
                let mut last_sequence = 0u64;

                loop {
                    tokio::select! {
                        result = socket.recv_from(&mut buf) => {
                            match result {
                                Ok((len, _addr)) => {
                                    if let Err(e) = Self::process_packet(
                                        &buf[..len],
                                        exchange,
                                        &orderbook_tx,
                                        &trades_tx,
                                        &mut last_sequence,
                                    ).await {
                                        warn!("Error processing packet: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("UDP receive error: {}", e);
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            info!("Shutting down UDP receiver for {}", exchange);
                            break;
                        }
                    }
                }
            });

            self.handles.push(handle);
        }

        Ok(())
    }

    async fn process_packet(
        data: &[u8],
        exchange: Exchange,
        orderbook_tx: &mpsc::Sender<OrderBookUpdate>,
        trades_tx: &mpsc::Sender<Trade>,
        last_sequence: &mut u64,
    ) -> Result<()> {
        // Parse header
        if data.len() < std::mem::size_of::<UdpPacketHeader>() {
            return Err(anyhow::anyhow!("Packet too small"));
        }

        let header = unsafe {
            &*(data.as_ptr() as *const UdpPacketHeader)
        };

        // Validate magic number
        const MAGIC: u32 = 0x48465450; // "HFTP"
        if header.magic != MAGIC {
            return Err(anyhow::anyhow!("Invalid magic number"));
        }

        // Check sequence number
        let seq = header.sequence; // Copy to avoid packed field issue
        if seq > 0 && seq != *last_sequence + 1 {
            warn!("Packet loss detected: expected {}, got {}",
                  *last_sequence + 1, seq);
        }
        *last_sequence = seq;

        // Extract symbol
        let symbol_start = std::mem::size_of::<UdpPacketHeader>();
        let symbol_end = symbol_start + header.symbol_len as usize;
        let symbol = std::str::from_utf8(&data[symbol_start..symbol_end])?
            .to_string();

        // Parse message based on type
        let payload = &data[symbol_end..];

        match header.message_type {
            0 => {
                // OrderBook update
                let update = Self::parse_orderbook(
                    payload,
                    exchange,
                    symbol,
                    header.timestamp
                )?;
                orderbook_tx.send(update).await?;
            }
            1 => {
                // Trade
                let trade = Self::parse_trade(
                    payload,
                    exchange,
                    symbol,
                    header.timestamp
                )?;
                trades_tx.send(trade).await?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown message type"));
            }
        }

        Ok(())
    }

    fn parse_orderbook(
        data: &[u8],
        exchange: Exchange,
        symbol: String,
        timestamp: Timestamp
    ) -> Result<OrderBookUpdate> {
        let mut offset = 0;

        // Read snapshot flag
        let is_snapshot = data[offset] != 0;
        offset += 1;

        // Read number of bid levels
        let bid_count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        let mut bids = Vec::with_capacity(bid_count);
        for _ in 0..bid_count {
            let price = f64::from_le_bytes(
                data[offset..offset + 8].try_into()?
            );
            offset += 8;
            let quantity = f64::from_le_bytes(
                data[offset..offset + 8].try_into()?
            );
            offset += 8;
            bids.push(PriceLevel { price, quantity });
        }

        // Read number of ask levels
        let ask_count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        let mut asks = Vec::with_capacity(ask_count);
        for _ in 0..ask_count {
            let price = f64::from_le_bytes(
                data[offset..offset + 8].try_into()?
            );
            offset += 8;
            let quantity = f64::from_le_bytes(
                data[offset..offset + 8].try_into()?
            );
            offset += 8;
            asks.push(PriceLevel { price, quantity });
        }

        Ok(OrderBookUpdate {
            exchange,
            symbol,
            timestamp,
            bids,
            asks,
            is_snapshot,
        })
    }

    fn parse_trade(
        data: &[u8],
        exchange: Exchange,
        symbol: String,
        timestamp: Timestamp
    ) -> Result<Trade> {
        let mut offset = 0;

        let price = f64::from_le_bytes(
            data[offset..offset + 8].try_into()?
        );
        offset += 8;

        let quantity = f64::from_le_bytes(
            data[offset..offset + 8].try_into()?
        );
        offset += 8;

        let side = if data[offset] == 0 {
            Side::Buy
        } else {
            Side::Sell
        };
        offset += 1;

        // Read trade ID length and string
        let id_len = data[offset] as usize;
        offset += 1;
        let trade_id = std::str::from_utf8(&data[offset..offset + id_len])?
            .to_string();

        Ok(Trade {
            exchange,
            symbol,
            timestamp,
            price,
            quantity,
            side,
            trade_id,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        for handle in self.handles.drain(..) {
            handle.await?;
        }

        Ok(())
    }
}