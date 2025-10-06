use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::Result;
use market_types::{Trade, OrderBookUpdate};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReceiverConfig {
    pub crypto_config: CryptoReceiverConfig,
    pub stock_config: StockReceiverConfig,
    pub buffer_size: usize,
    pub max_packet_size: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CryptoReceiverConfig {
    pub trade_multicast_addr: String,
    pub trade_multicast_port: u16,
    pub orderbook_multicast_addr: String,
    pub orderbook_multicast_port: u16,
    pub metadata_multicast_addr: String,
    pub metadata_multicast_port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StockReceiverConfig {
    pub trade_multicast_addr: String,
    pub trade_multicast_port: u16,
    pub orderbook_multicast_addr: String,
    pub orderbook_multicast_port: u16,
    pub metadata_multicast_addr: String,
    pub metadata_multicast_port: u16,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            crypto_config: CryptoReceiverConfig {
                trade_multicast_addr: "239.1.1.1".to_string(),
                trade_multicast_port: 5001,
                orderbook_multicast_addr: "239.1.1.2".to_string(),
                orderbook_multicast_port: 5002,
                metadata_multicast_addr: "239.1.1.3".to_string(),
                metadata_multicast_port: 5003,
            },
            stock_config: StockReceiverConfig {
                trade_multicast_addr: "239.2.1.1".to_string(),
                trade_multicast_port: 6001,
                orderbook_multicast_addr: "239.2.1.2".to_string(),
                orderbook_multicast_port: 6002,
                metadata_multicast_addr: "239.2.1.3".to_string(),
                metadata_multicast_port: 6003,
            },
            buffer_size: 65536,
            max_packet_size: 8192,
        }
    }
}

pub struct DatabaseUDPReceiver {
    config: ReceiverConfig,
    // TODO: Re-add when database crate is ready
    // questdb_client: Arc<RwLock<QuestDBClient>>,
    stats: Arc<RwLock<ReceiverStats>>,
}

#[derive(Debug, Default)]
struct ReceiverStats {
    crypto_trades_received: u64,
    crypto_orderbooks_received: u64,
    crypto_metadata_received: u64,
    stock_trades_received: u64,
    stock_orderbooks_received: u64,
    stock_metadata_received: u64,
    errors: u64,
}

impl DatabaseUDPReceiver {
    pub async fn new(
        config: ReceiverConfig,
    ) -> Result<Self> {
        Ok(Self {
            config,
            stats: Arc::new(RwLock::new(ReceiverStats::default())),
        })
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting Database UDP Receivers");
        
        // Start crypto receivers
        let crypto_trade_handle = self.start_crypto_trade_receiver();
        let crypto_orderbook_handle = self.start_crypto_orderbook_receiver();
        let crypto_metadata_handle = self.start_crypto_metadata_receiver();
        
        // Start stock receivers
        let stock_trade_handle = self.start_stock_trade_receiver();
        let stock_orderbook_handle = self.start_stock_orderbook_receiver();
        let stock_metadata_handle = self.start_stock_metadata_receiver();
        
        // Start stats reporter
        let stats_handle = self.start_stats_reporter();
        
        // Wait for all tasks
        tokio::select! {
            _ = crypto_trade_handle => error!("Crypto trade receiver stopped"),
            _ = crypto_orderbook_handle => error!("Crypto orderbook receiver stopped"),
            _ = crypto_metadata_handle => error!("Crypto metadata receiver stopped"),
            _ = stock_trade_handle => error!("Stock trade receiver stopped"),
            _ = stock_orderbook_handle => error!("Stock orderbook receiver stopped"),
            _ = stock_metadata_handle => error!("Stock metadata receiver stopped"),
            _ = stats_handle => error!("Stats reporter stopped"),
        }
        
        Ok(())
    }

    fn start_crypto_trade_receiver(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.crypto_config.clone();
        let stats = self.stats.clone();
        let buffer_size = self.config.buffer_size;
        
        tokio::spawn(async move {
            if let Err(e) = Self::receive_loop(
                &config.trade_multicast_addr,
                config.trade_multicast_port,
                buffer_size,
                move |data: &[u8]| {
                    let data = data.to_vec();
                    let stats = stats.clone();
                    Box::pin(async move {
                        if let Ok(_trade) = serde_json::from_slice::<Trade>(&data) {

                            // TODO: Implement feeder log methods
                            // if let Err(e) = client.write_crypto_trade(&trade).await {
                            //     error!("Failed to write crypto trade: {}", e);
                            // } else {
                                stats.write().await.crypto_trades_received += 1;
                            // }
                        }
                    })
                },
            ).await {
                error!("Crypto trade receiver error: {}", e);
            }
        })
    }

    fn start_crypto_orderbook_receiver(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.crypto_config.clone();
        let stats = self.stats.clone();
        let buffer_size = self.config.buffer_size;
        
        tokio::spawn(async move {
            if let Err(e) = Self::receive_loop(
                &config.orderbook_multicast_addr,
                config.orderbook_multicast_port,
                buffer_size,
                move |data: &[u8]| {
                    let data = data.to_vec();
                    let stats = stats.clone();
                    Box::pin(async move {
                        if let Ok(_orderbook) = serde_json::from_slice::<OrderBookUpdate>(&data) {

                            // TODO: Implement feeder log methods
                            // if let Err(e) = client.write_crypto_orderbook(&orderbook).await {
                            //     error!("Failed to write crypto orderbook: {}", e);
                            // } else {
                                stats.write().await.crypto_orderbooks_received += 1;
                            // }
                        }
                    })
                },
            ).await {
                error!("Crypto orderbook receiver error: {}", e);
            }
        })
    }

    fn start_crypto_metadata_receiver(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.crypto_config.clone();
        let stats = self.stats.clone();
        let buffer_size = self.config.buffer_size;

        tokio::spawn(async move {
            if let Err(e) = Self::receive_loop(
                &config.metadata_multicast_addr,
                config.metadata_multicast_port,
                buffer_size,
                move |data: &[u8]| {
                    let _data = data.to_vec();
                    let stats = stats.clone();
                    Box::pin(async move {
                        // TODO: Implement metadata writing when database crate is ready
                        stats.write().await.crypto_metadata_received += 1;
                    })
                },
            ).await {
                error!("Crypto metadata receiver error: {}", e);
            }
        })
    }

    fn start_stock_trade_receiver(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.stock_config.clone();
        let stats = self.stats.clone();
        let buffer_size = self.config.buffer_size;
        
        tokio::spawn(async move {
            if let Err(e) = Self::receive_loop(
                &config.trade_multicast_addr,
                config.trade_multicast_port,
                buffer_size,
                move |data: &[u8]| {
                    let data = data.to_vec();
                    let stats = stats.clone();
                    Box::pin(async move {
                        if let Ok(_trade) = serde_json::from_slice::<Trade>(&data) {

                            // TODO: Implement feeder log methods
                            // if let Err(e) = client.write_stock_trade(&trade).await {
                            //     error!("Failed to write stock trade: {}", e);
                            // } else {
                                stats.write().await.stock_trades_received += 1;
                            // }
                        }
                    })
                },
            ).await {
                error!("Stock trade receiver error: {}", e);
            }
        })
    }

    fn start_stock_orderbook_receiver(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.stock_config.clone();
        let stats = self.stats.clone();
        let buffer_size = self.config.buffer_size;
        
        tokio::spawn(async move {
            if let Err(e) = Self::receive_loop(
                &config.orderbook_multicast_addr,
                config.orderbook_multicast_port,
                buffer_size,
                move |data: &[u8]| {
                    let data = data.to_vec();
                    let stats = stats.clone();
                    Box::pin(async move {
                        if let Ok(_orderbook) = serde_json::from_slice::<OrderBookUpdate>(&data) {

                            // TODO: Implement feeder log methods
                            // if let Err(e) = client.write_stock_orderbook(&orderbook).await {
                            //     error!("Failed to write stock orderbook: {}", e);
                            // } else {
                                stats.write().await.stock_orderbooks_received += 1;
                            // }
                        }
                    })
                },
            ).await {
                error!("Stock orderbook receiver error: {}", e);
            }
        })
    }

    fn start_stock_metadata_receiver(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.stock_config.clone();
        let stats = self.stats.clone();
        let buffer_size = self.config.buffer_size;

        tokio::spawn(async move {
            if let Err(e) = Self::receive_loop(
                &config.metadata_multicast_addr,
                config.metadata_multicast_port,
                buffer_size,
                move |data: &[u8]| {
                    let _data = data.to_vec();
                    let stats = stats.clone();
                    Box::pin(async move {
                        // TODO: Implement metadata writing when database crate is ready
                        stats.write().await.stock_metadata_received += 1;
                    })
                },
            ).await {
                error!("Stock metadata receiver error: {}", e);
            }
        })
    }

    fn start_stats_reporter(&self) -> tokio::task::JoinHandle<()> {
        let stats = self.stats.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                let stats = stats.read().await;
                info!(
                    "Database Receiver Stats - Crypto: trades={}, orderbooks={}, metadata={} | Stock: trades={}, orderbooks={}, metadata={} | Errors={}",
                    stats.crypto_trades_received,
                    stats.crypto_orderbooks_received,
                    stats.crypto_metadata_received,
                    stats.stock_trades_received,
                    stats.stock_orderbooks_received,
                    stats.stock_metadata_received,
                    stats.errors
                );
            }
        })
    }

    async fn receive_loop<F, Fut>(
        multicast_addr: &str,
        port: u16,
        buffer_size: usize,
        handler: F,
    ) -> Result<()>
    where
        F: Fn(&[u8]) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port)).await?;
        
        // Join multicast group
        let multicast_addr: Ipv4Addr = multicast_addr.parse()
            .map_err(|e| anyhow::anyhow!("Invalid multicast address: {}", e))?;
        
        socket.join_multicast_v4(multicast_addr, Ipv4Addr::UNSPECIFIED)?;
        socket.set_multicast_loop_v4(false)?;
        
        info!("Listening on multicast {}:{}", multicast_addr, port);
        
        let mut buf = vec![0u8; buffer_size];
        
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, _addr)) => {
                    let data = &buf[..len];
                    handler(data).await;
                }
                Err(e) => {
                    error!("UDP receive error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
}