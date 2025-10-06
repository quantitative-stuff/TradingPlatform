//! Protocol codec utilities for encoding/decoding UDP packets

use market_types::{Trade, OrderBookUpdate, BinaryPacketHeader, MAGIC_NUMBER};
use anyhow::Result;
use serde_json;

/// Text format encoder (feeder format)
pub struct TextCodec;

impl TextCodec {
    pub fn encode_trade(trade: &Trade) -> Result<Vec<u8>> {
        let json = serde_json::to_string(trade)?;
        Ok(format!("TRADE|{}", json).into_bytes())
    }

    pub fn encode_orderbook(orderbook: &OrderBookUpdate) -> Result<Vec<u8>> {
        let json = serde_json::to_string(orderbook)?;
        Ok(format!("ORDERBOOK|{}", json).into_bytes())
    }

    pub fn decode(data: &[u8]) -> Result<PacketData> {
        let text = std::str::from_utf8(data)?;

        if let Some(payload) = text.strip_prefix("TRADE|") {
            let trade: Trade = serde_json::from_str(payload)?;
            Ok(PacketData::Trade(trade))
        } else if let Some(payload) = text.strip_prefix("ORDERBOOK|") {
            let orderbook: OrderBookUpdate = serde_json::from_str(payload)?;
            Ok(PacketData::OrderBook(orderbook))
        } else {
            Err(anyhow::anyhow!("Unknown packet format"))
        }
    }
}

/// Binary format encoder/decoder (pricing-platform format)
pub struct BinaryCodec;

impl BinaryCodec {
    pub fn encode_orderbook(
        orderbook: &OrderBookUpdate,
        sequence: u64,
    ) -> Result<Vec<u8>> {
        let mut packet = Vec::new();

        // Create header
        let header = BinaryPacketHeader {
            magic: MAGIC_NUMBER,
            sequence,
            timestamp: orderbook.timestamp,
            exchange_id: orderbook.exchange as u8,
            message_type: 0, // OrderBook
            symbol_len: orderbook.symbol.len() as u16,
        };

        // Serialize header (unsafe but fast)
        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                &header as *const _ as *const u8,
                std::mem::size_of::<BinaryPacketHeader>()
            )
        };
        packet.extend_from_slice(header_bytes);

        // Add symbol
        packet.extend_from_slice(orderbook.symbol.as_bytes());

        // Add orderbook data
        packet.push(if orderbook.is_snapshot { 1 } else { 0 });

        // Bids
        packet.extend_from_slice(&(orderbook.bids.len() as u16).to_le_bytes());
        for level in &orderbook.bids {
            packet.extend_from_slice(&level.price.to_le_bytes());
            packet.extend_from_slice(&level.quantity.to_le_bytes());
        }

        // Asks
        packet.extend_from_slice(&(orderbook.asks.len() as u16).to_le_bytes());
        for level in &orderbook.asks {
            packet.extend_from_slice(&level.price.to_le_bytes());
            packet.extend_from_slice(&level.quantity.to_le_bytes());
        }

        Ok(packet)
    }

    pub fn decode(data: &[u8]) -> Result<PacketData> {
        if data.len() < std::mem::size_of::<BinaryPacketHeader>() {
            return Err(anyhow::anyhow!("Packet too small"));
        }

        let header = unsafe {
            &*(data.as_ptr() as *const BinaryPacketHeader)
        };

        if header.magic != MAGIC_NUMBER {
            return Err(anyhow::anyhow!("Invalid magic number"));
        }

        // Parse based on message type
        match header.message_type {
            0 => {
                // OrderBook - implementation similar to receiver.rs
                // TODO: Complete implementation
                Err(anyhow::anyhow!("Binary orderbook decode not yet implemented"))
            }
            1 => {
                // Trade
                Err(anyhow::anyhow!("Binary trade decode not yet implemented"))
            }
            _ => Err(anyhow::anyhow!("Unknown message type"))
        }
    }
}

#[derive(Debug)]
pub enum PacketData {
    Trade(Trade),
    OrderBook(OrderBookUpdate),
}
