use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PacketType {
    Trade,
    OrderBook,
    Stats,
    Metadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpPacket {
    pub packet_type: PacketType,
    pub payload: Vec<u8>,
}

// Binary protocol header (from PricingPlatformRust)
#[repr(C, packed)]
pub struct BinaryPacketHeader {
    pub magic: u32,           // Magic number: 0x48465450 ("HFTP")
    pub sequence: u64,        // Sequence number
    pub timestamp: i64,       // Microseconds since epoch
    pub exchange_id: u8,      // Exchange identifier
    pub message_type: u8,     // 0: OrderBook, 1: Trade
    pub symbol_len: u16,      // Symbol string length
}

pub const MAGIC_NUMBER: u32 = 0x48465450; // "HFTP"
