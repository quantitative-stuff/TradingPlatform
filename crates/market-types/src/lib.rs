//! Shared market data types used across all components

pub mod exchange;
pub mod trade;
pub mod orderbook;
pub mod packet;
pub mod order_event;
pub mod features;
pub mod signal;
pub mod sequence;

pub use exchange::Exchange;
pub use trade::{Trade, Side};
pub use orderbook::{OrderBook, OrderBookUpdate, PriceLevel};
pub use packet::{UdpPacket, PacketType, BinaryPacketHeader, MAGIC_NUMBER};
pub use order_event::OrderEvent;
pub use features::{MarketFeatures, NBBO};
pub use signal::{Signal, SignalType};
pub use sequence::{SequenceInfo, SequenceExtractor, create_sequence_extractor};

pub type Timestamp = i64; // Microseconds since epoch
pub type Price = f64;
pub type Quantity = f64;
pub type Symbol = String;
