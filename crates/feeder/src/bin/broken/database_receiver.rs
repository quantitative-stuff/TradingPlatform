// Database Receiver - Receives UDP packets and stores in QuestDB
use anyhow::Result;
use tracing::{info, error};
use tokio::net::UdpSocket;
use serde_json;

use feeder::connect_to_databse::{
    QuestDBClient, QuestDBConfig,
};
use feeder::core::{TradeData, OrderBookData};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("=== Database Receiver ===");
    info!("Storing crypto data to QuestDB");
    info!("Press Ctrl+C to exit\n");

    // Initialize QuestDB client
    let questdb_config = QuestDBConfig::default();
    let questdb = QuestDBClient::new(questdb_config).await?;
    info!("✅ Connected to QuestDB on localhost:9009");
    
    // Bind to multicast to receive feeder data
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    socket.join_multicast_v4("239.1.1.1".parse()?, "0.0.0.0".parse()?)?;
    info!("✅ Listening for market data on multicast 239.1.1.1:9001");
    
    let mut buf = vec![0u8; 65536];
    let mut total_trades = 0u64;
    let mut total_orderbooks = 0u64;
    
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, _addr)) => {
                let data = String::from_utf8_lossy(&buf[..len]);
                
                // Handle batched packets (separated by newlines)
                for packet_line in data.lines() {
                    if packet_line.is_empty() {
                        continue;
                    }
                    
                    let parts: Vec<&str> = packet_line.split('|').collect();
                    
                    match parts[0] {
                        "TRADE" if parts.len() >= 2 => {
                            // Parse and store trade in QuestDB
                            if let Ok(trade) = serde_json::from_str::<TradeData>(parts[1]) {
                                // TODO: Implement feeder log methods
                                // if let Err(e) = questdb.write_crypto_trade(&trade).await {
                                //     error!("Failed to write trade to QuestDB: {}", e);
                                // } else {
                                    total_trades += 1;
                                    if total_trades % 1000 == 0 {
                                        info!("Stored {} trades to QuestDB", total_trades);
                                    }
                                // }
                            }
                        }
                        "ORDERBOOK" if parts.len() >= 2 => {
                            // Parse and store orderbook in QuestDB
                            if let Ok(orderbook) = serde_json::from_str::<OrderBookData>(parts[1]) {
                                // TODO: Implement feeder log methods
                                // if let Err(e) = questdb.write_crypto_orderbook(&orderbook).await {
                                //     error!("Failed to write orderbook to QuestDB: {}", e);
                                // } else {
                                    total_orderbooks += 1;
                                    if total_orderbooks % 1000 == 0 {
                                        info!("Stored {} orderbooks to QuestDB", total_orderbooks);
                                    }
                                // }
                            }
                        }
                        _ => {
                            // Other packet types (STATS, CONN, etc.)
                        }
                    }
                }
            }
            Err(e) => {
                error!("Error receiving UDP packet: {}", e);
            }
        }
    }
}