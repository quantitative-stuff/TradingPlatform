use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;
use tracing::{info, warn, error};
use anyhow::Result;
use market_types::{
    OrderBook, OrderBookUpdate, Exchange, SequenceExtractor,
    create_sequence_extractor
};

/// Maximum number of updates to buffer while waiting for snapshot
const MAX_BUFFER_SIZE: usize = 10000;

/// Maximum time to wait for snapshot before timing out
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(30);

/// Time between resync attempts
const RESYNC_BACKOFF_BASE: Duration = Duration::from_millis(100);

/// Maximum resync attempts before giving up
const MAX_RESYNC_ATTEMPTS: u32 = 5;

/// OrderBook builder state
#[derive(Debug, Clone)]
pub enum OrderBookState {
    /// Initial state - no data received yet
    Uninitialized,

    /// Buffering updates while waiting for snapshot
    Buffering {
        buffer: Vec<OrderBookUpdate>,
        started_at: Instant,
    },

    /// Fetching REST snapshot
    FetchingSnapshot {
        buffered_updates: Vec<OrderBookUpdate>,
        attempt: u32,
    },

    /// Processing snapshot and buffered updates
    Initializing {
        snapshot_seq: u64,
        buffered_updates: Vec<OrderBookUpdate>,
    },

    /// Normal operation - live updates
    Live {
        last_seq: u64,
        last_update: Instant,
        updates_since_sync: u64,
    },

    /// Resyncing after gap detected
    Resyncing {
        reason: ResyncReason,
        attempt: u32,
        started_at: Instant,
    },

    /// Error state
    Error {
        message: String,
        can_retry: bool,
    },
}

/// Reasons for resync
#[derive(Debug, Clone)]
pub enum ResyncReason {
    SequenceGap { expected: u64, actual: u64 },
    ChecksumMismatch,
    CrossedBook,
    Timeout,
    ManualRequest,
    InitialSync,
}

/// Statistics for monitoring
#[derive(Debug, Default, Clone)]
pub struct OrderBookStats {
    pub total_updates: u64,
    pub gaps_detected: u64,
    pub resyncs_triggered: u64,
    pub successful_resyncs: u64,
    pub failed_resyncs: u64,
    pub last_update_time: Option<Instant>,
    pub uptime: Duration,
}

/// OrderBook builder with state machine
pub struct OrderBookBuilder {
    // Identity
    symbol: String,
    exchange: Exchange,

    // Current state
    state: Arc<RwLock<OrderBookState>>,

    // OrderBook
    orderbook: Arc<RwLock<OrderBook>>,

    // Sequence extractor
    sequence_extractor: Box<dyn SequenceExtractor + Send + Sync>,

    // Channels
    update_receiver: mpsc::Receiver<OrderBookUpdate>,
    snapshot_sender: mpsc::Sender<OrderBook>,

    // REST client (if available) - for now we'll skip this
    // rest_client: Option<Arc<dyn ExchangeRestClient + Send + Sync>>,

    // Statistics
    stats: Arc<RwLock<OrderBookStats>>,

    // Configuration
    max_buffer_size: usize,
    snapshot_timeout: Duration,
    resync_backoff_base: Duration,
    max_resync_attempts: u32,
}

impl OrderBookBuilder {
    /// Create new orderbook builder
    pub fn new(
        symbol: String,
        exchange: Exchange,
        update_receiver: mpsc::Receiver<OrderBookUpdate>,
        snapshot_sender: mpsc::Sender<OrderBook>,
    ) -> Self {
        Self {
            symbol: symbol.clone(),
            exchange,
            state: Arc::new(RwLock::new(OrderBookState::Uninitialized)),
            orderbook: Arc::new(RwLock::new(OrderBook::new(
                symbol.clone(),
                exchange,
            ))),
            sequence_extractor: create_sequence_extractor(exchange),
            update_receiver,
            snapshot_sender,
            // rest_client: None,
            stats: Arc::new(RwLock::new(OrderBookStats::default())),
            max_buffer_size: MAX_BUFFER_SIZE,
            snapshot_timeout: SNAPSHOT_TIMEOUT,
            resync_backoff_base: RESYNC_BACKOFF_BASE,
            max_resync_attempts: MAX_RESYNC_ATTEMPTS,
        }
    }

    // REST client functionality removed for simplicity
    // Can be added back when we integrate with feeder crate

    /// Main processing loop
    pub async fn run(mut self) -> Result<()> {
        info!(
            "Starting orderbook builder for {} on {:?}",
            self.symbol, self.exchange
        );

        let start_time = Instant::now();

        // Start in buffering state
        {
            let mut state = self.state.write().await;
            *state = OrderBookState::Buffering {
                buffer: Vec::with_capacity(1000),
                started_at: Instant::now(),
            };
        }

        // Main processing loop
        loop {
            // Check current state
            let current_state = self.state.read().await.clone();

            match current_state {
                OrderBookState::Uninitialized => {
                    // Transition to buffering
                    let mut state = self.state.write().await;
                    *state = OrderBookState::Buffering {
                        buffer: Vec::with_capacity(1000),
                        started_at: Instant::now(),
                    };
                    info!("Transitioned to buffering state");
                }

                OrderBookState::Buffering { buffer, started_at } => {
                    // Check for timeout
                    if started_at.elapsed() > self.snapshot_timeout {
                        warn!("Buffering timeout, attempting resync");
                        self.trigger_resync(ResyncReason::Timeout).await?;
                        continue;
                    }

                    // Try to receive update with short timeout
                    match timeout(Duration::from_millis(100), self.update_receiver.recv()).await {
                        Ok(Some(update)) => {
                            self.handle_buffering_update(update, buffer).await?;
                        }
                        Ok(None) => {
                            error!("Update channel closed");
                            break;
                        }
                        Err(_) => {
                            // Timeout - check if we should fetch snapshot
                            if !buffer.is_empty() { // && self.rest_client.is_some() {
                                self.fetch_snapshot(buffer).await?;
                            }
                        }
                    }
                }

                OrderBookState::FetchingSnapshot {
                    buffered_updates,
                    attempt,
                } => {
                    if attempt > self.max_resync_attempts {
                        error!("Max snapshot attempts exceeded");
                        let mut state = self.state.write().await;
                        *state = OrderBookState::Error {
                            message: "Failed to fetch snapshot".to_string(),
                            can_retry: true,
                        };
                        continue;
                    }

                    // No REST client in this version, use first update as snapshot
                    if let Some(first_update) = buffered_updates.first() {
                        if first_update.is_snapshot {
                            self.apply_update(first_update).await?;
                            let mut state = self.state.write().await;
                            *state = OrderBookState::Live {
                                last_seq: first_update.update_id,
                                last_update: Instant::now(),
                                updates_since_sync: 1,
                            };
                        }
                    }
                }

                OrderBookState::Initializing {
                    snapshot_seq,
                    buffered_updates,
                } => {
                    // Apply buffered updates that come after snapshot
                    let mut applied_count = 0;
                    let mut dropped_count = 0;

                    for update in &buffered_updates {
                        if update.update_id > snapshot_seq {
                            self.apply_update(update).await?;
                            applied_count += 1;
                        } else {
                            dropped_count += 1;
                        }
                    }

                    info!(
                        "Initialization complete: applied {} updates, dropped {} stale",
                        applied_count, dropped_count
                    );

                    // Transition to live state
                    let book = self.orderbook.read().await;
                    let mut state = self.state.write().await;
                    *state = OrderBookState::Live {
                        last_seq: book.last_update_id,
                        last_update: Instant::now(),
                        updates_since_sync: applied_count,
                    };

                    // Send initial snapshot
                    let _ = self.snapshot_sender.send(book.clone()).await;
                }

                OrderBookState::Live {
                    last_seq,
                    last_update,
                    updates_since_sync,
                } => {
                    // Receive and process live updates
                    match self.update_receiver.recv().await {
                        Some(update) => {
                            // Check for sequence gap
                            if !update.is_snapshot && update.update_id != last_seq + 1 {
                                // Handle Binance-style range updates
                                let has_gap = if update.first_update_id > 0 {
                                    update.first_update_id != last_seq + 1
                                } else {
                                    update.update_id != last_seq + 1
                                };

                                if has_gap {
                                    warn!(
                                        "Sequence gap detected: expected {}, got {}",
                                        last_seq + 1,
                                        update.update_id
                                    );

                                    self.trigger_resync(ResyncReason::SequenceGap {
                                        expected: last_seq + 1,
                                        actual: update.update_id,
                                    })
                                    .await?;
                                    continue;
                                }
                            }

                            // Apply update
                            self.apply_update(&update).await?;

                            // Update state
                            let mut state = self.state.write().await;
                            *state = OrderBookState::Live {
                                last_seq: update.update_id,
                                last_update: Instant::now(),
                                updates_since_sync: updates_since_sync + 1,
                            };

                            // Periodically send snapshots
                            if updates_since_sync % 100 == 0 {
                                let book = self.orderbook.read().await;
                                let _ = self.snapshot_sender.send(book.clone()).await;
                            }

                            // Update stats
                            let mut stats = self.stats.write().await;
                            stats.total_updates += 1;
                            stats.last_update_time = Some(Instant::now());
                        }
                        None => {
                            error!("Update channel closed");
                            break;
                        }
                    }
                }

                OrderBookState::Resyncing {
                    reason,
                    attempt,
                    started_at,
                } => {
                    if attempt > self.max_resync_attempts {
                        error!("Max resync attempts exceeded");
                        let mut state = self.state.write().await;
                        *state = OrderBookState::Error {
                            message: format!("Resync failed: {:?}", reason),
                            can_retry: true,
                        };
                        continue;
                    }

                    // Wait with backoff
                    tokio::time::sleep(self.resync_backoff_base * attempt).await;

                    // Reset to buffering state
                    let mut state = self.state.write().await;
                    *state = OrderBookState::Buffering {
                        buffer: Vec::with_capacity(1000),
                        started_at: Instant::now(),
                    };

                    info!("Restarting from buffering state after resync");
                }

                OrderBookState::Error { message, can_retry } => {
                    error!("OrderBook builder in error state: {}", message);
                    if can_retry {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let mut state = self.state.write().await;
                        *state = OrderBookState::Uninitialized;
                    } else {
                        break;
                    }
                }
            }

            // Update uptime
            let mut stats = self.stats.write().await;
            stats.uptime = start_time.elapsed();
        }

        Ok(())
    }

    /// Handle update while in buffering state
    async fn handle_buffering_update(
        &self,
        update: OrderBookUpdate,
        mut buffer: Vec<OrderBookUpdate>,
    ) -> Result<()> {
        // Check buffer size
        if buffer.len() >= self.max_buffer_size {
            warn!("Buffer full, dropping oldest updates");
            buffer.drain(0..100); // Drop oldest 100 updates
        }

        let is_snapshot = update.is_snapshot;
        buffer.push(update);

        // Update state with new buffer
        let mut state = self.state.write().await;
        if let OrderBookState::Buffering { started_at, .. } = *state {
            *state = OrderBookState::Buffering {
                buffer: buffer.clone(),
                started_at,
            };

            // If this is first update and it's a snapshot, process it
            if buffer.len() == 1 && is_snapshot {
                drop(state); // Release lock before async operation
                self.fetch_snapshot(buffer).await?;
            }
        }

        Ok(())
    }

    /// Fetch REST snapshot
    async fn fetch_snapshot(&self, buffered_updates: Vec<OrderBookUpdate>) -> Result<()> {
        info!("Fetching REST snapshot for {}", self.symbol);

        let mut state = self.state.write().await;
        *state = OrderBookState::FetchingSnapshot {
            buffered_updates,
            attempt: 1,
        };

        let mut stats = self.stats.write().await;
        stats.resyncs_triggered += 1;

        Ok(())
    }

    // REST snapshot functionality will be added when we integrate REST clients
    // For now, this is commented out to allow compilation
    /*
    /// Apply REST snapshot
    async fn apply_snapshot(
        &self,
        snapshot: crate::OrderBookSnapshot,
        buffered_updates: Vec<OrderBookUpdate>,
    ) -> Result<()> {
        info!(
            "Applying snapshot with last_update_id: {:?}",
            snapshot.last_update_id
        );

        let mut book = self.orderbook.write().await;

        // Clear current book
        book.bids.clear();
        book.asks.clear();

        // Apply snapshot
        book.last_update_id = snapshot.last_update_id.unwrap_or(0);
        book.timestamp = snapshot.timestamp as i64;

        for (price, quantity) in snapshot.bids {
            book.bids.push(market_types::PriceLevel { price, quantity });
        }

        for (price, quantity) in snapshot.asks {
            book.asks.push(market_types::PriceLevel { price, quantity });
        }

        // Sort levels
        book.bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
        book.asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

        let snapshot_seq = book.last_update_id;
        drop(book); // Release lock

        // Transition to initializing
        let mut state = self.state.write().await;
        *state = OrderBookState::Initializing {
            snapshot_seq,
            buffered_updates,
        };

        let mut stats = self.stats.write().await;
        stats.successful_resyncs += 1;

        Ok(())
    }
    */

    /// Apply an update to the orderbook
    async fn apply_update(&self, update: &OrderBookUpdate) -> Result<()> {
        let mut book = self.orderbook.write().await;

        // Update sequence
        book.last_update_id = update.update_id;
        book.timestamp = update.timestamp;
        book.update_count += 1;

        // If it's a snapshot, replace entire book
        if update.is_snapshot {
            book.bids = update.bids.clone();
            book.asks = update.asks.clone();
        } else {
            // Apply incremental updates
            // For simplicity, we'll replace the levels
            // In production, you'd merge them properly
            for level in &update.bids {
                // Remove existing level and add new one
                book.bids.retain(|l| (l.price - level.price).abs() > f64::EPSILON);
                if level.quantity > 0.0 {
                    book.bids.push(level.clone());
                }
            }

            for level in &update.asks {
                // Remove existing level and add new one
                book.asks.retain(|l| (l.price - level.price).abs() > f64::EPSILON);
                if level.quantity > 0.0 {
                    book.asks.push(level.clone());
                }
            }

            // Re-sort
            book.bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
            book.asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
        }

        Ok(())
    }

    /// Trigger resync
    async fn trigger_resync(&self, reason: ResyncReason) -> Result<()> {
        warn!("Triggering resync due to: {:?}", reason);

        let mut state = self.state.write().await;
        *state = OrderBookState::Resyncing {
            reason,
            attempt: 1,
            started_at: Instant::now(),
        };

        let mut stats = self.stats.write().await;
        stats.gaps_detected += 1;
        stats.resyncs_triggered += 1;

        Ok(())
    }

    /// Get current state
    pub async fn get_state(&self) -> OrderBookState {
        self.state.read().await.clone()
    }

    /// Get statistics
    pub async fn get_stats(&self) -> OrderBookStats {
        self.stats.read().await.clone()
    }

    /// Get current orderbook snapshot
    pub async fn get_orderbook(&self) -> OrderBook {
        self.orderbook.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_state_transitions() {
        let (tx, rx) = mpsc::channel(100);
        let (snapshot_tx, _snapshot_rx) = mpsc::channel(10);

        let builder = OrderBookBuilder::new(
            "BTCUSDT".to_string(),
            Exchange::Binance,
            rx,
            snapshot_tx,
        );

        // Check initial state
        let state = builder.get_state().await;
        assert!(matches!(state, OrderBookState::Uninitialized));

        // Start builder in background
        let builder_handle = tokio::spawn(async move {
            let _ = builder.run().await;
        });

        // Send an update
        let update = OrderBookUpdate {
            exchange: Exchange::Binance,
            symbol: "BTCUSDT".to_string(),
            timestamp: 1000000,
            bids: vec![],
            asks: vec![],
            is_snapshot: true,
            update_id: 100,
            first_update_id: 100,
            prev_update_id: None,
        };

        tx.send(update).await.unwrap();

        // Give it time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Clean up
        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), builder_handle).await;
    }
}