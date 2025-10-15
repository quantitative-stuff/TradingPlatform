/// Persistence layer for orderbook state
///
/// Provides functionality to save and restore orderbook state to/from disk
/// for recovery after crashes or restarts.

use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::io::{Write as IoWrite, Read};
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};
use market_types::{OrderBook, OrderBookUpdate, Exchange};
use crate::orderbook_builder::OrderBookState;
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// Persistence configuration
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// Base directory for persistence files
    pub base_dir: PathBuf,

    /// Save snapshots every N updates
    pub snapshot_interval: u64,

    /// Keep last N snapshots
    pub max_snapshots: usize,

    /// Enable compression (Windows: use NTFS compression)
    pub compress: bool,

    /// Write to temp file first then rename (atomic on Windows)
    pub atomic_writes: bool,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("./orderbook_snapshots"),
            snapshot_interval: 1000,
            max_snapshots: 10,
            compress: false,
            atomic_writes: true,
        }
    }
}

/// Persisted orderbook state
#[derive(Debug, Serialize, Deserialize)]
pub struct PersistedState {
    /// Timestamp when saved
    pub timestamp: u64,

    /// Exchange
    pub exchange: Exchange,

    /// Symbol
    pub symbol: String,

    /// Orderbook state
    pub orderbook: OrderBook,

    /// Last update sequence number
    pub last_sequence: u64,

    /// Buffered updates (if any)
    pub buffered_updates: Vec<OrderBookUpdate>,

    /// Checksum of the state
    pub checksum: Option<String>,

    /// Version for backward compatibility
    pub version: u32,
}

/// Persistence manager for orderbook state
pub struct PersistenceManager {
    config: PersistenceConfig,
    symbol: String,
    exchange: Exchange,
    updates_since_snapshot: u64,
    snapshot_dir: PathBuf,
}

impl PersistenceManager {
    /// Create new persistence manager
    pub fn new(
        config: PersistenceConfig,
        symbol: String,
        exchange: Exchange,
    ) -> Result<Self> {
        // Create snapshot directory
        let snapshot_dir = config.base_dir
            .join(format!("{:?}", exchange).to_lowercase())
            .join(&symbol);

        fs::create_dir_all(&snapshot_dir)
            .context("Failed to create snapshot directory")?;

        // On Windows, optionally enable NTFS compression
        #[cfg(windows)]
        if config.compress {
            Self::enable_ntfs_compression(&snapshot_dir);
        }

        Ok(Self {
            config,
            symbol,
            exchange,
            updates_since_snapshot: 0,
            snapshot_dir,
        })
    }

    /// Save orderbook state to disk
    pub fn save_state(&mut self, orderbook: &OrderBook, sequence: u64) -> Result<()> {
        self.updates_since_snapshot += 1;

        // Check if we should create a snapshot
        if self.updates_since_snapshot < self.config.snapshot_interval {
            return Ok(());
        }

        let state = PersistedState {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            exchange: self.exchange,
            symbol: self.symbol.clone(),
            orderbook: orderbook.clone(),
            last_sequence: sequence,
            buffered_updates: Vec::new(),
            checksum: None, // Could add checksum here
            version: 1,
        };

        // Generate filename with timestamp
        let filename = format!(
            "snapshot_{}_{}_{}.json",
            self.symbol,
            sequence,
            state.timestamp
        );

        let filepath = self.snapshot_dir.join(&filename);

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&state)
            .context("Failed to serialize state")?;

        // Write to file (atomic if configured)
        if self.config.atomic_writes {
            self.atomic_write(&filepath, json.as_bytes())?;
        } else {
            fs::write(&filepath, json)
                .context("Failed to write snapshot")?;
        }

        info!("Saved orderbook snapshot: {}", filename);

        // Reset counter
        self.updates_since_snapshot = 0;

        // Clean up old snapshots
        self.cleanup_old_snapshots()?;

        Ok(())
    }

    /// Load most recent orderbook state
    pub fn load_latest_state(&self) -> Result<Option<PersistedState>> {
        // Find snapshot files
        let snapshots = self.find_snapshots()?;

        if snapshots.is_empty() {
            info!("No snapshots found for {}", self.symbol);
            return Ok(None);
        }

        // Try loading from most recent to oldest
        for snapshot_path in snapshots.iter().rev() {
            match self.load_snapshot(snapshot_path) {
                Ok(state) => {
                    info!(
                        "Loaded snapshot from {:?}, sequence: {}",
                        snapshot_path.file_name().unwrap(),
                        state.last_sequence
                    );
                    return Ok(Some(state));
                }
                Err(e) => {
                    warn!("Failed to load snapshot {:?}: {}", snapshot_path, e);
                }
            }
        }

        Ok(None)
    }

    /// Load specific snapshot
    fn load_snapshot(&self, path: &Path) -> Result<PersistedState> {
        let json = fs::read_to_string(path)
            .context("Failed to read snapshot file")?;

        let state: PersistedState = serde_json::from_str(&json)
            .context("Failed to deserialize snapshot")?;

        // Validate version
        if state.version > 1 {
            warn!("Snapshot version {} is newer than supported", state.version);
        }

        Ok(state)
    }

    /// Find all snapshot files
    fn find_snapshots(&self) -> Result<Vec<PathBuf>> {
        let mut snapshots = Vec::new();

        for entry in fs::read_dir(&self.snapshot_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(name) = path.file_name() {
                    if name.to_string_lossy().starts_with("snapshot_") {
                        snapshots.push(path);
                    }
                }
            }
        }

        // Sort by modification time (oldest first)
        snapshots.sort_by_key(|p| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });

        Ok(snapshots)
    }

    /// Clean up old snapshots
    fn cleanup_old_snapshots(&self) -> Result<()> {
        let snapshots = self.find_snapshots()?;

        if snapshots.len() <= self.config.max_snapshots {
            return Ok(());
        }

        let to_remove = snapshots.len() - self.config.max_snapshots;

        for path in snapshots.iter().take(to_remove) {
            debug!("Removing old snapshot: {:?}", path.file_name());
            fs::remove_file(path)
                .context("Failed to remove old snapshot")?;
        }

        Ok(())
    }

    /// Atomic write (write to temp file then rename)
    fn atomic_write(&self, path: &Path, data: &[u8]) -> Result<()> {
        let temp_path = path.with_extension("tmp");

        // Write to temp file
        fs::write(&temp_path, data)
            .context("Failed to write temp file")?;

        // Rename to final path (atomic on Windows with MOVEFILE_REPLACE_EXISTING)
        #[cfg(windows)]
        {
            use std::os::windows::fs as winfs;
            // This is atomic on NTFS
            fs::rename(&temp_path, path)
                .context("Failed to rename temp file")?;
        }

        #[cfg(not(windows))]
        {
            fs::rename(&temp_path, path)
                .context("Failed to rename temp file")?;
        }

        Ok(())
    }

    /// Enable NTFS compression on Windows
    #[cfg(windows)]
    fn enable_ntfs_compression(dir: &Path) {
        // Note: Full implementation would use FILE_ATTRIBUTE_COMPRESSED
        // Simplified for compilation
        debug!("NTFS compression would be enabled on {:?}", dir);
    }

    /// Save buffered updates for recovery
    pub fn save_buffer(&self, buffer: &VecDeque<OrderBookUpdate>) -> Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }

        let buffer_file = self.snapshot_dir.join("buffer.json");
        let json = serde_json::to_string(&buffer)
            .context("Failed to serialize buffer")?;

        fs::write(&buffer_file, json)
            .context("Failed to write buffer")?;

        debug!("Saved {} buffered updates", buffer.len());
        Ok(())
    }

    /// Load buffered updates
    pub fn load_buffer(&self) -> Result<VecDeque<OrderBookUpdate>> {
        let buffer_file = self.snapshot_dir.join("buffer.json");

        if !buffer_file.exists() {
            return Ok(VecDeque::new());
        }

        let json = fs::read_to_string(&buffer_file)
            .context("Failed to read buffer file")?;

        let buffer: VecDeque<OrderBookUpdate> = serde_json::from_str(&json)
            .context("Failed to deserialize buffer")?;

        // Delete buffer file after loading
        fs::remove_file(&buffer_file)?;

        debug!("Loaded {} buffered updates", buffer.len());
        Ok(buffer)
    }
}

/// Recovery helper for orderbook builder
pub struct RecoveryManager {
    persistence: PersistenceManager,
}

impl RecoveryManager {
    pub fn new(persistence: PersistenceManager) -> Self {
        Self { persistence }
    }

    /// Attempt to recover orderbook state
    pub fn recover(&self) -> Result<Option<(OrderBook, u64, VecDeque<OrderBookUpdate>)>> {
        // Load latest snapshot
        let state = match self.persistence.load_latest_state()? {
            Some(s) => s,
            None => return Ok(None),
        };

        // Load any buffered updates
        let buffer = self.persistence.load_buffer()
            .unwrap_or_else(|e| {
                warn!("Failed to load buffer: {}", e);
                VecDeque::new()
            });

        info!(
            "Recovered orderbook at sequence {}, {} buffered updates",
            state.last_sequence,
            buffer.len()
        );

        Ok(Some((state.orderbook, state.last_sequence, buffer)))
    }

    /// Save current state for recovery
    pub fn checkpoint(
        &mut self,
        orderbook: &OrderBook,
        sequence: u64,
        buffer: &VecDeque<OrderBookUpdate>,
    ) -> Result<()> {
        // Save orderbook state
        self.persistence.save_state(orderbook, sequence)?;

        // Save buffer if not empty
        if !buffer.is_empty() {
            self.persistence.save_buffer(buffer)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_types::PriceLevel;
    use tempfile::TempDir;

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            base_dir: temp_dir.path().to_path_buf(),
            snapshot_interval: 1,
            max_snapshots: 3,
            compress: false,
            atomic_writes: true,
        };

        let mut pm = PersistenceManager::new(
            config,
            "BTCUSDT".to_string(),
            Exchange::Binance,
        ).unwrap();

        // Create test orderbook
        let mut orderbook = OrderBook::new("BTCUSDT".to_string(), Exchange::Binance);
        orderbook.bids.push(PriceLevel { price: 100.0, quantity: 10.0 });
        orderbook.asks.push(PriceLevel { price: 101.0, quantity: 10.0 });
        orderbook.last_update_id = 123;

        // Save state
        pm.save_state(&orderbook, 123).unwrap();

        // Load state
        let loaded = pm.load_latest_state().unwrap().unwrap();
        assert_eq!(loaded.last_sequence, 123);
        assert_eq!(loaded.orderbook.last_update_id, 123);
        assert_eq!(loaded.orderbook.bids.len(), 1);
        assert_eq!(loaded.orderbook.asks.len(), 1);
    }

    #[test]
    fn test_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            base_dir: temp_dir.path().to_path_buf(),
            snapshot_interval: 1,
            max_snapshots: 2,
            compress: false,
            atomic_writes: false,
        };

        let mut pm = PersistenceManager::new(
            config,
            "BTCUSDT".to_string(),
            Exchange::Binance,
        ).unwrap();

        let mut orderbook = OrderBook::new("BTCUSDT".to_string(), Exchange::Binance);

        // Save multiple snapshots
        for i in 1..=5 {
            orderbook.last_update_id = i * 100;
            pm.save_state(&orderbook, i * 100).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Should only have 2 snapshots left
        let snapshots = pm.find_snapshots().unwrap();
        assert_eq!(snapshots.len(), 2);

        // Should be the most recent ones
        let loaded = pm.load_latest_state().unwrap().unwrap();
        assert_eq!(loaded.last_sequence, 500);
    }
}