/// Binance REST Snapshot Fetcher for Orderbook Synchronization
///
/// According to official Binance documentation:
/// https://developers.binance.com/docs/derivatives/usds-margined-futures/websocket-market-streams/How-to-manage-a-local-order-book-correctly
///
/// Binance WebSocket sends ONLY incremental updates (no initial snapshot)
/// We MUST fetch initial snapshot from REST API and synchronize with WebSocket events

use serde_json::Value;
use tracing::{debug, info, warn, error};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;

/// Binance depth snapshot from REST API
#[derive(Debug, Clone)]
pub struct BinanceSnapshot {
    pub symbol: String,
    pub last_update_id: u64,
    pub bids: Vec<(String, String)>,  // (price, qty) as strings
    pub asks: Vec<(String, String)>,
}

/// Buffered WebSocket event waiting for synchronization
#[derive(Debug, Clone)]
pub struct BufferedDepthEvent {
    pub first_update_id: u64,  // U
    pub final_update_id: u64,  // u
    pub prev_update_id: Option<u64>,  // pu
    pub data: Value,
}

/// Manages orderbook snapshot fetching and event synchronization for one symbol
pub struct BinanceOrderbookSynchronizer {
    pub symbol: String,
    pub asset_type: String,
    pub snapshot: Arc<RwLock<Option<BinanceSnapshot>>>,
    pub event_buffer: Arc<RwLock<VecDeque<BufferedDepthEvent>>>,
    pub is_synchronized: Arc<RwLock<bool>>,
    pub last_validated_update_id: Arc<RwLock<u64>>,
}

impl BinanceOrderbookSynchronizer {
    pub fn new(symbol: String, asset_type: String) -> Self {
        Self {
            symbol,
            asset_type,
            snapshot: Arc::new(RwLock::new(None)),
            event_buffer: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            is_synchronized: Arc::new(RwLock::new(false)),
            last_validated_update_id: Arc::new(RwLock::new(0)),
        }
    }

    /// Fetch initial snapshot from Binance REST API
    pub async fn fetch_snapshot(&self) -> Result<BinanceSnapshot, String> {
        let url = match self.asset_type.as_str() {
            "spot" => format!(
                "https://api.binance.com/api/v3/depth?symbol={}&limit=1000",
                self.symbol
            ),
            "futures" => format!(
                "https://fapi.binance.com/fapi/v1/depth?symbol={}&limit=1000",
                self.symbol
            ),
            _ => return Err(format!("Invalid asset type: {}", self.asset_type)),
        };

        info!("Fetching Binance {} snapshot for {} from: {}", self.asset_type, self.symbol, url);

        // Use reqwest with retry logic
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let mut retry_count = 0;
        let max_retries = 3;

        loop {
            match client.get(&url).send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let error_text = response.text().await.unwrap_or_default();
                        error!("Binance REST API error {}: {}", status, error_text);

                        retry_count += 1;
                        if retry_count >= max_retries {
                            return Err(format!("HTTP {}: {}", status, error_text));
                        }

                        warn!("Retrying snapshot fetch for {} (attempt {}/{})",
                            self.symbol, retry_count + 1, max_retries);
                        tokio::time::sleep(Duration::from_millis(500 * retry_count)).await;
                        continue;
                    }

                    let json: Value = response.json().await
                        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

                    let last_update_id = json.get("lastUpdateId")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| "Missing lastUpdateId in snapshot".to_string())?;

                    let bids: Vec<(String, String)> = json.get("bids")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| "Missing bids in snapshot".to_string())?
                        .iter()
                        .filter_map(|arr| {
                            let arr = arr.as_array()?;
                            if arr.len() >= 2 {
                                Some((
                                    arr[0].as_str()?.to_string(),
                                    arr[1].as_str()?.to_string(),
                                ))
                            } else {
                                None
                            }
                        })
                        .collect();

                    let asks: Vec<(String, String)> = json.get("asks")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| "Missing asks in snapshot".to_string())?
                        .iter()
                        .filter_map(|arr| {
                            let arr = arr.as_array()?;
                            if arr.len() >= 2 {
                                Some((
                                    arr[0].as_str()?.to_string(),
                                    arr[1].as_str()?.to_string(),
                                ))
                            } else {
                                None
                            }
                        })
                        .collect();

                    info!("✅ Binance snapshot fetched for {}: lastUpdateId={}, {} bids, {} asks",
                        self.symbol, last_update_id, bids.len(), asks.len());

                    let snapshot = BinanceSnapshot {
                        symbol: self.symbol.clone(),
                        last_update_id,
                        bids,
                        asks,
                    };

                    // Store snapshot
                    {
                        let mut snapshot_lock = self.snapshot.write().await;
                        *snapshot_lock = Some(snapshot.clone());
                    }

                    return Ok(snapshot);
                }
                Err(e) => {
                    error!("Failed to fetch snapshot for {}: {}", self.symbol, e);

                    retry_count += 1;
                    if retry_count >= max_retries {
                        return Err(format!("Network error after {} retries: {}", max_retries, e));
                    }

                    warn!("Retrying snapshot fetch for {} (attempt {}/{})",
                        self.symbol, retry_count + 1, max_retries);
                    tokio::time::sleep(Duration::from_millis(500 * retry_count)).await;
                }
            }
        }
    }

    /// Buffer a WebSocket depth event (before synchronization)
    pub async fn buffer_event(&self, event: BufferedDepthEvent) {
        let mut buffer = self.event_buffer.write().await;

        // Limit buffer size to prevent memory issues
        if buffer.len() >= 10000 {
            warn!("Binance event buffer for {} is full (10000 events), dropping oldest",
                self.symbol);
            buffer.pop_front();
        }

        // Save values for debug before moving
        let first_update_id = event.first_update_id;
        let final_update_id = event.final_update_id;
        let buffer_size = buffer.len() + 1; // +1 for the event we're about to add

        buffer.push_back(event);
        debug!("Buffered Binance event for {}: U={}, u={}, buffer_size={}",
            self.symbol, first_update_id, final_update_id, buffer_size);
    }

    /// Synchronize buffered events with snapshot according to Binance docs
    ///
    /// Official procedure:
    /// 1. Drop any event where u <= lastUpdateId
    /// 2. First processed event must satisfy: U <= lastUpdateId AND u >= lastUpdateId
    /// 3. Validate continuity: each event's pu must equal prior event's u
    pub async fn synchronize(&self) -> Result<Vec<Value>, String> {
        let snapshot = self.snapshot.read().await;
        let snapshot = snapshot.as_ref()
            .ok_or_else(|| "No snapshot available for synchronization".to_string())?;

        let mut buffer = self.event_buffer.write().await;
        let last_update_id = snapshot.last_update_id;

        info!("Synchronizing {} buffered events for {} (snapshot lastUpdateId={})",
            buffer.len(), self.symbol, last_update_id);

        // Step 1: Drop events where u <= lastUpdateId
        let before_count = buffer.len();
        buffer.retain(|event| event.final_update_id > last_update_id);
        let dropped = before_count - buffer.len();

        if dropped > 0 {
            info!("Dropped {} old events for {} (u <= {})", dropped, self.symbol, last_update_id);
        }

        if buffer.is_empty() {
            warn!("No buffered events after filtering for {} - waiting for new events", self.symbol);
            return Err("No valid events in buffer after filtering".to_string());
        }

        // Step 2: Validate first event
        let first_event = buffer.front()
            .ok_or_else(|| "Buffer is empty".to_string())?;

        // Extract values before any mutable borrows
        let first_u = first_event.first_update_id;
        let first_final_u = first_event.final_update_id;

        if !(first_u <= last_update_id && first_final_u >= last_update_id) {
            error!("❌ Binance sync failed for {}: First event (U={}, u={}) doesn't overlap with snapshot (lastUpdateId={})",
                self.symbol, first_u, first_final_u, last_update_id);

            // Clear buffer and request resync
            buffer.clear();
            return Err(format!(
                "First event doesn't overlap with snapshot. Expected U<={} AND u>={}, got U={}, u={}",
                last_update_id, last_update_id,
                first_u, first_final_u
            ));
        }

        info!("✅ First event validation passed for {}: U={}, u={} overlaps with lastUpdateId={}",
            self.symbol, first_u, first_final_u, last_update_id);

        // Step 3: Collect validated events
        let mut validated_events = Vec::new();
        let mut prev_final_update_id = last_update_id;

        for event in buffer.drain(..) {
            // Validate sequence continuity using pu field
            if let Some(pu) = event.prev_update_id {
                if pu != prev_final_update_id {
                    error!("❌ Sequence gap detected for {}: expected pu={}, got pu={}",
                        self.symbol, prev_final_update_id, pu);

                    // Gap detected - need to resync
                    validated_events.clear();
                    return Err(format!(
                        "Sequence gap: expected pu={}, got pu={}. Resync required.",
                        prev_final_update_id, pu
                    ));
                }
            }

            validated_events.push(event.data.clone());
            prev_final_update_id = event.final_update_id;
        }

        info!("✅ Binance orderbook synchronized for {}: {} events validated",
            self.symbol, validated_events.len());

        // Mark as synchronized
        {
            let mut sync_flag = self.is_synchronized.write().await;
            *sync_flag = true;
        }

        {
            let mut last_id = self.last_validated_update_id.write().await;
            *last_id = prev_final_update_id;
        }

        Ok(validated_events)
    }

    /// Validate a new incoming event (after synchronization)
    pub async fn validate_event(&self, event: &BufferedDepthEvent) -> bool {
        let is_synced = *self.is_synchronized.read().await;

        if !is_synced {
            // Not synchronized yet, buffer it
            return false;
        }

        let last_id = *self.last_validated_update_id.read().await;

        // Check sequence continuity
        if let Some(pu) = event.prev_update_id {
            if pu != last_id {
                error!("❌ Sequence gap for {}: expected pu={}, got pu={}. Marking for resync.",
                    self.symbol, last_id, pu);

                // Mark as not synchronized - will trigger resync
                let mut sync_flag = self.is_synchronized.write().await;
                *sync_flag = false;

                return false;
            }
        }

        // Update last validated ID
        {
            let mut last_id_lock = self.last_validated_update_id.write().await;
            *last_id_lock = event.final_update_id;
        }

        true
    }

    /// Check if synchronization is needed
    pub async fn needs_sync(&self) -> bool {
        !(*self.is_synchronized.read().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_snapshot_validation() {
        let synchronizer = BinanceOrderbookSynchronizer::new(
            "BTCUSDT".to_string(),
            "futures".to_string(),
        );

        // Create mock snapshot
        let snapshot = BinanceSnapshot {
            symbol: "BTCUSDT".to_string(),
            last_update_id: 100,
            bids: vec![],
            asks: vec![],
        };

        {
            let mut snap_lock = synchronizer.snapshot.write().await;
            *snap_lock = Some(snapshot);
        }

        // Test event that overlaps correctly: U=95, u=105 (overlaps with lastUpdateId=100)
        let valid_event = BufferedDepthEvent {
            first_update_id: 95,
            final_update_id: 105,
            prev_update_id: Some(99),
            data: serde_json::json!({}),
        };

        synchronizer.buffer_event(valid_event).await;

        // Should succeed
        assert!(synchronizer.synchronize().await.is_ok());
    }
}
