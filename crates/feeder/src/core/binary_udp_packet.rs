use std::mem;
use crate::core::{TradeData, OrderBookData};

/// Binary UDP packet format as specified in docs/api/udp_packet.md
/// All multi-byte integers use network byte order (Big Endian)

/// 58-byte packet header (fixed size, no sequence number for low latency)
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct PacketHeader {
    pub protocol_version: u8,      // 1B - Protocol version (currently 1)
    pub exchange_timestamp: u64,   // 8B - Exchange event timestamp (Unix nanoseconds)
    pub local_timestamp: u64,      // 8B - Feeder send timestamp (Unix nanoseconds)
    pub flags_and_count: u8,       // 1B - Flags and item count bitfield
    pub symbol: [u8; 20],          // 20B - Symbol name, UTF-8, null-terminated
    pub exchange: [u8; 20],        // 20B - Exchange name, UTF-8, null-terminated
} // Total: 58 bytes

/// 16-byte OrderBook item for binary payload
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct OrderBookItem {
    pub price: i64,       // 8B - Scaled by 10^8
    pub quantity: i64,    // 8B - Scaled by 10^8
} // Total: 16 bytes

/// 32-byte Trade item for binary payload
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct TradeItem {
    pub trade_id: u64,       // 8B - Trade unique ID
    pub price: i64,          // 8B - Scaled by 10^8
    pub quantity: i64,       // 8B - Scaled by 10^8
    pub side: u8,            // 1B - 0: Sell, 1: Buy
    pub reserved: [u8; 7],   // 7B - Reserved for future use
} // Total: 32 bytes

impl PacketHeader {
    /// Create new packet header with default values
    pub fn new() -> Self {
        PacketHeader {
            protocol_version: 1,
            exchange_timestamp: 0,
            local_timestamp: 0,
            flags_and_count: 0,
            symbol: [0; 20],
            exchange: [0; 20],
        }
    }

    /// Extract is_last flag (bit 7)
    pub fn is_last(&self) -> bool {
        (self.flags_and_count & 0b1000_0000) != 0
    }

    /// Extract packet_type flag (bits 6-4) - 1: Trade, 2: OrderBook
    pub fn packet_type(&self) -> u8 {
        (self.flags_and_count & 0b0111_0000) >> 4
    }

    /// Extract is_bid flag (bit 3) - OrderBook only: 0: Ask, 1: Bid
    pub fn is_bid(&self) -> bool {
        (self.flags_and_count & 0b0000_1000) != 0
    }

    /// Extract item_count (bits 2-0) - Max 7 items per packet
    pub fn item_count(&self) -> u8 {
        self.flags_and_count & 0b0000_0111
    }

    /// Set flags and item count
    pub fn set_flags_and_count(&mut self, is_last: bool, packet_type: u8, is_bid: bool, count: u8) {
        let mut flags = 0u8;

        // Set is_last flag (bit 7)
        if is_last {
            flags |= 0b1000_0000;
        }

        // Set packet_type (bits 6-4, 3 bits for types 0-7)
        let packet_type_bits = (packet_type & 0x07) << 4;
        flags |= packet_type_bits;

        // Set is_bid flag (bit 3)
        if is_bid {
            flags |= 0b0000_1000;
        }

        // Set item count (bits 2-0, max 7)
        let item_count = count & 0b0000_0111;

        self.flags_and_count = flags | item_count;
    }

    /// Set symbol name (max 19 chars + null terminator)
    pub fn set_symbol(&mut self, symbol: &str) {
        self.symbol = [0; 20];
        let bytes = symbol.as_bytes();
        let len = std::cmp::min(bytes.len(), 19); // Leave room for null terminator
        self.symbol[..len].copy_from_slice(&bytes[..len]);
    }

    /// Set exchange name (max 19 chars + null terminator)
    pub fn set_exchange(&mut self, exchange: &str) {
        self.exchange = [0; 20];
        let bytes = exchange.as_bytes();
        let len = std::cmp::min(bytes.len(), 19); // Leave room for null terminator
        self.exchange[..len].copy_from_slice(&bytes[..len]);
    }

    /// Convert header to network byte order (Big Endian)
    pub fn to_network_order(&mut self) {
        self.exchange_timestamp = self.exchange_timestamp.to_be();
        self.local_timestamp = self.local_timestamp.to_be();
    }

    /// Convert header from network byte order to host order
    pub fn from_network_order(&mut self) {
        self.exchange_timestamp = u64::from_be(self.exchange_timestamp);
        self.local_timestamp = u64::from_be(self.local_timestamp);
    }
}

impl OrderBookItem {
    /// Create new OrderBook item with scaled price and quantity
    pub fn new(price: f64, quantity: f64) -> Self {
        OrderBookItem {
            price: (price * 100_000_000.0) as i64,    // Scale by 10^8
            quantity: (quantity * 100_000_000.0) as i64, // Scale by 10^8
        }
    }

    /// Convert to network byte order
    pub fn to_network_order(&mut self) {
        self.price = self.price.to_be();
        self.quantity = self.quantity.to_be();
    }

    /// Convert from network byte order
    pub fn from_network_order(&mut self) {
        self.price = i64::from_be(self.price);
        self.quantity = i64::from_be(self.quantity);
    }

    /// Get actual price (unscaled)
    pub fn get_price(&self) -> f64 {
        self.price as f64 / 100_000_000.0
    }

    /// Get actual quantity (unscaled)
    pub fn get_quantity(&self) -> f64 {
        self.quantity as f64 / 100_000_000.0
    }
}

impl TradeItem {
    /// Create new Trade item
    pub fn new(trade_id: u64, price: f64, quantity: f64, is_buy: bool) -> Self {
        TradeItem {
            trade_id,
            price: (price * 100_000_000.0) as i64,    // Scale by 10^8
            quantity: (quantity * 100_000_000.0) as i64, // Scale by 10^8
            side: if is_buy { 1 } else { 0 },
            reserved: [0; 7],
        }
    }

    /// Convert to network byte order
    pub fn to_network_order(&mut self) {
        self.trade_id = self.trade_id.to_be();
        self.price = self.price.to_be();
        self.quantity = self.quantity.to_be();
    }

    /// Convert from network byte order
    pub fn from_network_order(&mut self) {
        self.trade_id = u64::from_be(self.trade_id);
        self.price = i64::from_be(self.price);
        self.quantity = i64::from_be(self.quantity);
    }

    /// Get actual price (unscaled)
    pub fn get_price(&self) -> f64 {
        self.price as f64 / 100_000_000.0
    }

    /// Get actual quantity (unscaled)
    pub fn get_quantity(&self) -> f64 {
        self.quantity as f64 / 100_000_000.0
    }

    /// Check if this is a buy trade
    pub fn is_buy(&self) -> bool {
        self.side == 1
    }
}

/// Complete binary packet ready for UDP transmission
pub struct BinaryUdpPacket {
    pub header: PacketHeader,
    pub payload: Vec<u8>,
}

impl BinaryUdpPacket {
    /// Create new empty packet
    pub fn new() -> Self {
        BinaryUdpPacket {
            header: PacketHeader::new(),
            payload: Vec::new(),
        }
    }

    /// Create trade packet from TradeData
    pub fn from_trade(trade: &TradeData) -> Self {
        let mut packet = BinaryUdpPacket::new();

        // Set header - handle timestamp conversion safely
        // If timestamp is already in nanoseconds (> 1e15), use as-is
        // If in milliseconds (< 1e13), convert to nanoseconds
        packet.header.exchange_timestamp = if trade.timestamp > 1_000_000_000_000_000 {
            trade.timestamp as u64 // Already in nanoseconds
        } else {
            (trade.timestamp.saturating_mul(1_000_000)) as u64 // Convert ms to ns with overflow protection
        };
        packet.header.local_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        packet.header.set_symbol(&trade.symbol);
        packet.header.set_exchange(&trade.exchange);
        packet.header.set_flags_and_count(true, 1, false, 1); // is_last=true, packet_type=1 (Trade), is_bid=false, count=1

        // Create trade item
        let mut trade_item = TradeItem::new(
            0, // trade_id - could be derived from timestamp
            trade.price,
            trade.quantity,
            trade.quantity > 0.0, // Simple buy/sell detection
        );
        trade_item.to_network_order();

        // Add trade item to payload
        let trade_bytes = unsafe {
            std::slice::from_raw_parts(
                &trade_item as *const TradeItem as *const u8,
                mem::size_of::<TradeItem>()
            )
        };
        packet.payload.extend_from_slice(trade_bytes);

        packet
    }

    /// Create orderbook packet from OrderBookData
    pub fn from_orderbook(orderbook: &OrderBookData, is_bid: bool) -> Self {
        let mut packet = BinaryUdpPacket::new();

        // Set header - handle timestamp conversion safely
        // If timestamp is already in nanoseconds (> 1e15), use as-is
        // If in milliseconds (< 1e13), convert to nanoseconds
        packet.header.exchange_timestamp = if orderbook.timestamp > 1_000_000_000_000_000 {
            orderbook.timestamp as u64 // Already in nanoseconds
        } else {
            (orderbook.timestamp.saturating_mul(1_000_000)) as u64 // Convert ms to ns with overflow protection
        };
        packet.header.local_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        packet.header.set_symbol(&orderbook.symbol);
        packet.header.set_exchange(&orderbook.exchange);

        // Choose bids or asks
        let levels = if is_bid { &orderbook.bids } else { &orderbook.asks };
        let count = std::cmp::min(levels.len(), 7) as u8; // Max 7 items per packet

        packet.header.set_flags_and_count(true, 2, is_bid, count); // is_last=true, packet_type=2 (OrderBook), is_bid, count

        // Add orderbook items to payload
        for &(price, quantity) in levels.iter().take(7) {
            let mut item = OrderBookItem::new(price, quantity);
            item.to_network_order();

            let item_bytes = unsafe {
                std::slice::from_raw_parts(
                    &item as *const OrderBookItem as *const u8,
                    mem::size_of::<OrderBookItem>()
                )
            };
            packet.payload.extend_from_slice(item_bytes);
        }

        packet
    }

    /// Convert packet to bytes for UDP transmission
    pub fn to_bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(58 + self.payload.len());

        // Convert header to network order
        self.header.to_network_order();

        // Serialize header
        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                &self.header as *const PacketHeader as *const u8,
                mem::size_of::<PacketHeader>()
            )
        };
        bytes.extend_from_slice(header_bytes);

        // Add payload
        bytes.extend_from_slice(&self.payload);

        bytes
    }

    /// Get packet size in bytes
    pub fn size(&self) -> usize {
        58 + self.payload.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_header_size() {
        assert_eq!(mem::size_of::<PacketHeader>(), 58);
    }

    #[test]
    fn test_orderbook_item_size() {
        assert_eq!(mem::size_of::<OrderBookItem>(), 16);
    }

    #[test]
    fn test_trade_item_size() {
        assert_eq!(mem::size_of::<TradeItem>(), 32);
    }

    #[test]
    fn test_flags_and_count() {
        let mut header = PacketHeader::new();
        header.set_flags_and_count(true, 1, false, 15);

        assert!(header.is_last());
        assert_eq!(header.packet_type(), 1);
        assert!(!header.is_bid());
        assert_eq!(header.item_count(), 15);
    }

    #[test]
    fn test_scaling() {
        let item = OrderBookItem::new(95000.0, 1.5);
        assert_eq!(item.get_price(), 95000.0);
        assert_eq!(item.get_quantity(), 1.5);
    }
}