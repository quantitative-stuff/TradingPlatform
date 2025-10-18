// Per-symbol orderbook builder for Deribit following local_orderbook_builder_plan.md
// Lock-free architecture using mpsc channels

use tokio::sync::mpsc;
use crate::core::{OrderBookData, MultiPortUdpSender};
use crate::load_config::TimestampUnit;
use std::sync::Arc;
use tracing::{debug, warn};

const USE_MULTI_PORT_UDP: bool = true;

/// Orderbook update message sent via channel
#[derive(Clone, Debug)]
pub struct DeribitOrderBookUpdate {
    pub bids: Vec<(String, i64, i64)>,  // (action, price, qty) - action = "new", "change", "delete"
    pub asks: Vec<(String, i64, i64)>,  // (action, price, qty)
    pub timestamp: u64,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub timestamp_unit: TimestampUnit,
}

/// Per-symbol orderbook builder task
/// Maintains local orderbook state and applies updates sequentially
pub struct DeribitOrderBookBuilder {
    exchange: String,
    symbol: String,
    asset_type: String,
    orderbook: Option<OrderBookData>,
}

impl DeribitOrderBookBuilder {
    pub fn new(
        exchange: String,
        symbol: String,
        asset_type: String,
    ) -> Self {
        Self {
            exchange,
            symbol,
            asset_type,
            orderbook: None,
        }
    }

    /// Process an orderbook update with action-based changes
    pub fn process_update(&mut self, update: DeribitOrderBookUpdate) {
        debug!("[Deribit] [{}] Processing update with {} bid updates, {} ask updates",
            self.symbol, update.bids.len(), update.asks.len());

        // Initialize orderbook if it doesn't exist
        if self.orderbook.is_none() {
            self.orderbook = Some(OrderBookData {
                exchange: self.exchange.clone(),
                symbol: self.symbol.clone(),
                asset_type: self.asset_type.clone(),
                bids: Vec::new(),
                asks: Vec::new(),
                price_precision: update.price_precision,
                quantity_precision: update.quantity_precision,
                timestamp: update.timestamp,
                timestamp_unit: update.timestamp_unit,
            });
        }

        if let Some(orderbook) = &mut self.orderbook {
            // Apply bid updates
            for (action, price, qty) in update.bids {
                match action.as_str() {
                    "new" | "change" => {
                        // Update or insert
                        if let Some(entry) = orderbook.bids.iter_mut().find(|(p, _)| *p == price) {
                            entry.1 = qty; // Update quantity
                        } else {
                            orderbook.bids.push((price, qty)); // Insert new level
                        }
                    }
                    "delete" => {
                        // Delete this price level
                        orderbook.bids.retain(|(p, _)| *p != price);
                    }
                    _ => {}
                }
            }

            // Apply ask updates
            for (action, price, qty) in update.asks {
                match action.as_str() {
                    "new" | "change" => {
                        // Update or insert
                        if let Some(entry) = orderbook.asks.iter_mut().find(|(p, _)| *p == price) {
                            entry.1 = qty; // Update quantity
                        } else {
                            orderbook.asks.push((price, qty)); // Insert new level
                        }
                    }
                    "delete" => {
                        // Delete this price level
                        orderbook.asks.retain(|(p, _)| *p != price);
                    }
                    _ => {}
                }
            }

            // Sort orderbooks (bids descending, asks ascending)
            orderbook.bids.sort_by(|a, b| b.0.cmp(&a.0)); // Descending
            orderbook.asks.sort_by(|a, b| a.0.cmp(&b.0)); // Ascending

            // Update timestamp
            orderbook.timestamp = update.timestamp;

            // Clone for publishing (to avoid borrow checker issues)
            let orderbook_clone = orderbook.clone();
            drop(orderbook); // Explicitly drop mutable borrow

            self.publish_orderbook(&orderbook_clone);
        }
    }

    /// Publish orderbook to UDP and global state
    fn publish_orderbook(&self, orderbook: &OrderBookData) {
        // Store in global ORDERBOOKS for backwards compatibility
        {
            let mut global_orderbooks = crate::core::ORDERBOOKS.write();
            global_orderbooks.push(orderbook.clone());
        }

        // Send UDP packet - dynamically get sender
        if USE_MULTI_PORT_UDP {
            if let Some(sender) = crate::core::get_multi_port_sender() {
                let _ = sender.send_orderbook_data(orderbook.clone());
            } else if let Some(sender) = crate::core::get_binary_udp_sender() {
                let _ = sender.send_orderbook_data(orderbook.clone());
            }
        } else {
            if let Some(sender) = crate::core::get_binary_udp_sender() {
                let _ = sender.send_orderbook_data(orderbook.clone());
            }
        }

        debug!("[Deribit] [{}] Published orderbook with {} bids, {} asks",
            self.symbol, orderbook.bids.len(), orderbook.asks.len());

        crate::core::COMPARE_NOTIFY.notify_waiters();
    }
}

/// Spawn per-symbol orderbook builder task
/// Returns the sender channel for sending updates to this symbol's task
pub fn spawn_orderbook_builder(
    exchange: String,
    symbol: String,
    asset_type: String,
) -> mpsc::Sender<DeribitOrderBookUpdate> {
    let (tx, mut rx) = mpsc::channel::<DeribitOrderBookUpdate>(1000);

    let symbol_clone = symbol.clone();
    tokio::spawn(async move {
        let mut builder = DeribitOrderBookBuilder::new(
            exchange,
            symbol,
            asset_type,
        );

        debug!("[Deribit] Orderbook builder task started for {}", symbol_clone);

        while let Some(update) = rx.recv().await {
            builder.process_update(update);
        }

        debug!("[Deribit] Orderbook builder task stopped for {}", symbol_clone);
    });

    tx
}
