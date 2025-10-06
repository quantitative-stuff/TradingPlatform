use market_types::{
    OrderBook, OrderBookUpdate, OrderEvent, PriceLevel, Side, Timestamp
};
use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;
use parking_lot::RwLock;

pub struct MatchingEngine {
    order_books: Arc<DashMap<String, Arc<RwLock<OrderBook>>>>,
    order_id_counter: Arc<RwLock<u64>>,
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self {
            order_books: Arc::new(DashMap::new()),
            order_id_counter: Arc::new(RwLock::new(0)),
        }
    }

    /// Process L2 order book update and generate L3 events
    pub fn process_l2_update(
        &self,
        update: &OrderBookUpdate
    ) -> Result<Vec<OrderEvent>> {
        let mut events = Vec::new();

        // Get or create order book for this symbol
        let book = self.order_books
            .entry(update.symbol.clone())
            .or_insert_with(|| {
                Arc::new(RwLock::new(OrderBook::new(
                    update.symbol.clone(),
                    update.exchange
                )))
            })
            .clone();

        let mut book_guard = book.write();

        if update.is_snapshot {
            // Clear existing book on snapshot
            events.extend(self.generate_clear_events(&book_guard, update.timestamp));
            book_guard.bids.clear();
            book_guard.asks.clear();
        }

        // Process bid updates
        events.extend(self.process_side_updates(
            &mut book_guard.bids,
            &update.bids,
            Side::Buy,
            update.timestamp
        )?);

        // Process ask updates
        events.extend(self.process_side_updates(
            &mut book_guard.asks,
            &update.asks,
            Side::Sell,
            update.timestamp
        )?);

        book_guard.timestamp = update.timestamp;

        Ok(events)
    }

    fn process_side_updates(
        &self,
        current_levels: &mut Vec<PriceLevel>,
        update_levels: &[PriceLevel],
        side: Side,
        timestamp: Timestamp
    ) -> Result<Vec<OrderEvent>> {
        let mut events = Vec::new();

        for update_level in update_levels {
            // Find existing level
            let existing_idx = current_levels
                .iter()
                .position(|l| (l.price - update_level.price).abs() < f64::EPSILON);

            match existing_idx {
                Some(idx) => {
                    let current_qty = current_levels[idx].quantity;
                    let new_qty = update_level.quantity;

                    if new_qty == 0.0 {
                        // Level removed - generate cancel events
                        events.push(self.generate_cancel_event(
                            side,
                            update_level.price,
                            current_qty,
                            timestamp
                        ));
                        current_levels.remove(idx);
                    } else if new_qty > current_qty {
                        // Quantity increased - generate add event
                        events.push(self.generate_add_event(
                            side,
                            update_level.price,
                            new_qty - current_qty,
                            timestamp
                        ));
                        current_levels[idx].quantity = new_qty;
                    } else if new_qty < current_qty {
                        // Quantity decreased - could be cancel or execute
                        events.push(self.generate_reduce_event(
                            side,
                            update_level.price,
                            current_qty - new_qty,
                            timestamp
                        ));
                        current_levels[idx].quantity = new_qty;
                    }
                }
                None if update_level.quantity > 0.0 => {
                    // New level - generate add event
                    events.push(self.generate_add_event(
                        side,
                        update_level.price,
                        update_level.quantity,
                        timestamp
                    ));

                    // Insert in sorted order
                    let insert_pos = match side {
                        Side::Buy => {
                            // Bids sorted descending
                            current_levels
                                .binary_search_by(|l| {
                                    update_level.price.partial_cmp(&l.price).unwrap()
                                })
                                .unwrap_or_else(|e| e)
                        }
                        Side::Sell => {
                            // Asks sorted ascending
                            current_levels
                                .binary_search_by(|l| {
                                    l.price.partial_cmp(&update_level.price).unwrap()
                                })
                                .unwrap_or_else(|e| e)
                        }
                    };

                    current_levels.insert(insert_pos, update_level.clone());
                }
                _ => {} // Ignore zero quantity updates for non-existent levels
            }
        }

        Ok(events)
    }

    fn generate_add_event(
        &self,
        side: Side,
        price: f64,
        quantity: f64,
        timestamp: Timestamp
    ) -> OrderEvent {
        let order_id = self.get_next_order_id();
        OrderEvent::Add {
            order_id,
            side,
            price,
            quantity,
            timestamp,
        }
    }

    fn generate_cancel_event(
        &self,
        _side: Side,
        _price: f64,
        _quantity: f64,
        timestamp: Timestamp
    ) -> OrderEvent {
        let order_id = self.get_next_order_id();
        OrderEvent::Cancel {
            order_id,
            timestamp,
        }
    }

    fn generate_reduce_event(
        &self,
        _side: Side,
        _price: f64,
        quantity: f64,
        timestamp: Timestamp
    ) -> OrderEvent {
        let order_id = self.get_next_order_id();
        // We assume reduction is due to execution
        OrderEvent::Execute {
            order_id,
            executed_quantity: quantity,
            timestamp,
        }
    }

    fn generate_clear_events(
        &self,
        book: &OrderBook,
        timestamp: Timestamp
    ) -> Vec<OrderEvent> {
        let mut events = Vec::new();

        // Generate cancel events for all existing orders
        for level in &book.bids {
            if level.quantity > 0.0 {
                events.push(self.generate_cancel_event(
                    Side::Buy,
                    level.price,
                    level.quantity,
                    timestamp
                ));
            }
        }

        for level in &book.asks {
            if level.quantity > 0.0 {
                events.push(self.generate_cancel_event(
                    Side::Sell,
                    level.price,
                    level.quantity,
                    timestamp
                ));
            }
        }

        events
    }

    fn get_next_order_id(&self) -> u64 {
        let mut counter = self.order_id_counter.write();
        *counter += 1;
        *counter
    }

    pub fn get_order_book(&self, symbol: &str) -> Option<OrderBook> {
        self.order_books
            .get(symbol)
            .map(|book_ref| book_ref.read().clone())
    }
}