//! Unified UDP protocol for market data transmission
//!
//! This module provides both sender and receiver implementations for UDP multicast.
//!
//! ## Protocol Variants
//!
//! - **Text Format** (from feeder): `TRADE|{json}`, `ORDERBOOK|{json}`
//!   - Used for debugging and database logging
//!   - Human-readable, easier to debug
//!
//! - **Binary Format** (from pricing-platform): Custom binary protocol with header
//!   - Used for low-latency trading
//!   - Compact, efficient parsing
//!
//! ## Architecture
//!
//! ```text
//! Feeder → UDP Sender (text/binary) → Multicast Group
//!                                          ↓
//!                                      Receivers:
//!                                      - Database Receiver (text)
//!                                      - Trading Platform (binary)
//! ```

pub mod sender;
pub mod receiver;
pub mod database_receiver;
pub mod codec;

pub use sender::{UdpSender, init_global_udp_sender, get_udp_sender};
pub use receiver::UdpReceiver;
pub use database_receiver::DatabaseUDPReceiver;
