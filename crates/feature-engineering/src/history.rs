use market_types::{OrderBook, Trade, OrderEvent, Timestamp};
use std::collections::VecDeque;

/// Maintains historical order book snapshots for time-based features
pub struct OrderBookHistory {
    snapshots: VecDeque<OrderBook>,
    max_lookback_micros: i64,
}

impl OrderBookHistory {
    pub fn new(max_lookback_ms: i64) -> Self {
        Self {
            snapshots: VecDeque::new(),
            max_lookback_micros: max_lookback_ms * 1000,
        }
    }

    pub fn add_snapshot(&mut self, book: OrderBook) {
        self.snapshots.push_back(book);
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if let Some(latest) = self.snapshots.back() {
            let cutoff = latest.timestamp - self.max_lookback_micros;
            while let Some(oldest) = self.snapshots.front() {
                if oldest.timestamp < cutoff {
                    self.snapshots.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    pub fn get_book_at(&self, timestamp_offset_ms: i64) -> Option<&OrderBook> {
        let target_time = self.snapshots.back()?.timestamp - (timestamp_offset_ms * 1000);

        self.snapshots.iter()
            .rev()
            .find(|book| book.timestamp <= target_time)
    }

    pub fn latest(&self) -> Option<&OrderBook> {
        self.snapshots.back()
    }
}

/// Maintains trade history for flow-based features
pub struct TradeHistory {
    trades: VecDeque<Trade>,
    max_lookback_micros: i64,
}

impl TradeHistory {
    pub fn new(max_lookback_ms: i64) -> Self {
        Self {
            trades: VecDeque::new(),
            max_lookback_micros: max_lookback_ms * 1000,
        }
    }

    pub fn add_trade(&mut self, trade: Trade) {
        self.trades.push_back(trade);
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if let Some(latest) = self.trades.back() {
            let cutoff = latest.timestamp - self.max_lookback_micros;
            while let Some(oldest) = self.trades.front() {
                if oldest.timestamp < cutoff {
                    self.trades.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    pub fn trades_since(&self, lookback_ms: i64) -> impl Iterator<Item = &Trade> {
        let cutoff = self.trades.back()
            .map(|t| t.timestamp - lookback_ms * 1000)
            .unwrap_or(0);

        self.trades.iter().filter(move |t| t.timestamp >= cutoff)
    }
}

/// Maintains order event history for OFI calculation
pub struct OrderEventHistory {
    events: VecDeque<OrderEvent>,
    max_lookback_micros: i64,
}

impl OrderEventHistory {
    pub fn new(max_lookback_ms: i64) -> Self {
        Self {
            events: VecDeque::new(),
            max_lookback_micros: max_lookback_ms * 1000,
        }
    }

    pub fn add_event(&mut self, event: OrderEvent) {
        self.events.push_back(event);
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if let Some(latest) = self.events.back() {
            let cutoff = Self::event_timestamp(latest) - self.max_lookback_micros;
            while let Some(oldest) = self.events.front() {
                if Self::event_timestamp(oldest) < cutoff {
                    self.events.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    fn event_timestamp(event: &OrderEvent) -> Timestamp {
        match event {
            OrderEvent::Add { timestamp, .. } => *timestamp,
            OrderEvent::Modify { timestamp, .. } => *timestamp,
            OrderEvent::Cancel { timestamp, .. } => *timestamp,
            OrderEvent::Execute { timestamp, .. } => *timestamp,
        }
    }

    pub fn events_since(&self, lookback_ms: i64) -> impl Iterator<Item = &OrderEvent> {
        let cutoff = self.events.back()
            .map(|e| Self::event_timestamp(e) - lookback_ms * 1000)
            .unwrap_or(0);

        self.events.iter().filter(move |e| Self::event_timestamp(e) >= cutoff)
    }
}
