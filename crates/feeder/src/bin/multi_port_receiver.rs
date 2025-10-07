/// Multi-Port UDP Receiver (20 Parallel Streams)
///
/// Receives market data from 20 multicast addresses in parallel:
/// - Binance: 8 receiver threads (4 trade + 4 orderbook)
/// - Others: 2 receiver threads each (1 trade + 1 orderbook)
///
/// Usage:
///   cargo run --bin multi_port_receiver

use feeder::core::multi_port_udp_receiver::{MultiPortUdpReceiver, DataType};
use feeder::core::binary_udp_packet::{PacketHeader, OrderBookItem, TradeItem};
use tokio::signal;
use tracing::{info, error, warn, debug};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("╔════════════════════════════════════════════════════════╗");
    info!("║      Multi-Port UDP Receiver - 20 Parallel Streams    ║");
    info!("╠════════════════════════════════════════════════════════╣");
    info!("║  Binance:  8 addresses (4 trade + 4 orderbook)        ║");
    info!("║  Bybit:    2 addresses (1 trade + 1 orderbook)        ║");
    info!("║  OKX:      2 addresses (1 trade + 1 orderbook)        ║");
    info!("║  Deribit:  2 addresses (1 trade + 1 orderbook)        ║");
    info!("║  Upbit:    2 addresses (1 trade + 1 orderbook)        ║");
    info!("║  Coinbase: 2 addresses (1 trade + 1 orderbook)        ║");
    info!("║  Bithumb:  2 addresses (1 trade + 1 orderbook)        ║");
    info!("║  ──────────────────────────────────────────────────    ║");
    info!("║  Total:    20 receiver threads running in parallel    ║");
    info!("╚════════════════════════════════════════════════════════╝");

    // Create receiver with packet handler
    let receiver = MultiPortUdpReceiver::new(|data, exchange, receiver_id, data_type| async move {
        // Parse binary packet
        if data.len() < 58 {
            warn!("Received packet too small from {}: {} bytes", receiver_id, data.len());
            return;
        }

        // Parse header (58 bytes)
        let header = parse_packet_header(&data);

        match data_type {
            DataType::Trade => {
                if let Some(trade) = parse_trade_packet(&data, &header) {
                    debug!(
                        "[{}] Trade: {} @ ${:.2} x {:.4}",
                        exchange,
                        parse_string(&header.symbol),
                        trade.price,
                        trade.quantity,
                    );
                }
            }
            DataType::OrderBook => {
                if let Some(levels) = parse_orderbook_packet(&data, &header) {
                    debug!(
                        "[{}] OrderBook: {} - {} levels ({})",
                        exchange,
                        parse_string(&header.symbol),
                        levels,
                        if header.is_bid() { "BIDS" } else { "ASKS" }
                    );
                }
            }
        }
    }).await?;

    info!("✅ Multi-Port Receiver started successfully");
    info!("Listening on 20 multicast addresses...");
    info!("Press Ctrl+C to stop");

    // Wait for Ctrl+C (don't print stats in loop to avoid move issue)
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Shutting down...");
        }
    }

    // Print final stats
    receiver.print_stats();
    receiver.abort_all();

    info!("Receiver stopped");
    Ok(())
}

/// Parse packet header from raw bytes
fn parse_packet_header(data: &[u8]) -> PacketHeader {
    unsafe {
        let mut header: PacketHeader = std::ptr::read(data.as_ptr() as *const PacketHeader);
        header.from_network_order();
        header
    }
}

/// Parse null-terminated string from header field
fn parse_string(bytes: &[u8]) -> String {
    let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..null_pos]).to_string()
}

/// Parse trade packet
fn parse_trade_packet(data: &[u8], header: &PacketHeader) -> Option<TradeData> {
    if data.len() < 58 + 32 {
        return None;
    }

    let item_count = header.item_count() as usize;
    if item_count == 0 {
        return None;
    }

    unsafe {
        let mut trade_item: TradeItem = std::ptr::read((data.as_ptr().add(58)) as *const TradeItem);
        trade_item.from_network_order();

        Some(TradeData {
            price: trade_item.get_price(),
            quantity: trade_item.get_quantity(),
        })
    }
}

/// Parse orderbook packet (just returns count of levels)
fn parse_orderbook_packet(data: &[u8], header: &PacketHeader) -> Option<usize> {
    let item_count = header.item_count() as usize;
    if item_count == 0 || data.len() < 58 + (item_count * 16) {
        return None;
    }

    Some(item_count)
}

#[derive(Debug)]
struct TradeData {
    price: f64,
    quantity: f64,
}
