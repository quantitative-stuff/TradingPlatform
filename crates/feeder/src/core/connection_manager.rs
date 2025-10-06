use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub max_symbols_per_connection: usize,
    pub connection_timeout_seconds: u64,
    pub reconnect_delay_seconds: u64,
    pub max_reconnect_attempts: u32,
}

#[derive(Debug, Clone)]
pub struct ConnectionStatus {
    pub connection_id: String,
    pub asset_type: String,
    pub stream_type: String,
    pub symbols: Vec<String>,
    pub is_connected: bool,
    pub last_connected: Option<Instant>,
    pub last_message: Option<Instant>,
    pub reconnect_attempts: u32,
    pub total_messages: u64,
    pub errors: Vec<String>,
}

#[derive(Debug)]
pub struct ConnectionManager {
    config: ConnectionConfig,
    connections: Arc<RwLock<HashMap<String, ConnectionStatus>>>,
    connection_count: Arc<Mutex<u32>>,
}

impl ConnectionManager {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            connection_count: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn create_connection(
        &self,
        asset_type: &str,
        stream_type: &str,
        symbols: Vec<String>,
    ) -> String {
        let mut count = self.connection_count.lock().await;
        *count += 1;
        let connection_id = format!("{}_{}_{}_{}", asset_type, stream_type, *count, chrono::Utc::now().timestamp());

        let symbols_len = symbols.len();

        let status = ConnectionStatus {
            connection_id: connection_id.clone(),
            asset_type: asset_type.to_string(),
            stream_type: stream_type.to_string(),
            symbols,
            is_connected: false,
            last_connected: None,
            last_message: None,
            reconnect_attempts: 0,
            total_messages: 0,
            errors: Vec::new(),
        };

        {
            let mut connections = self.connections.write().await;
            connections.insert(connection_id.clone(), status);
        }

        info!("Created connection {} for {} {} with {} symbols", 
              connection_id, asset_type, stream_type, symbols_len);
        
        connection_id
    }

    pub async fn update_connection_status(
        &self,
        connection_id: &str,
        is_connected: bool,
        error: Option<String>,
    ) {
        let mut connections = self.connections.write().await;
        if let Some(status) = connections.get_mut(connection_id) {
            status.is_connected = is_connected;
            if is_connected {
                status.last_connected = Some(Instant::now());
                status.reconnect_attempts = 0;
                info!("Connection {} established", connection_id);
            } else {
                if let Some(err) = error {
                    warn!("Connection {} lost: {}", connection_id, &err);
                    status.errors.push(err);
                }
            }
        }
    }

    pub async fn record_message(&self, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(status) = connections.get_mut(connection_id) {
            status.last_message = Some(Instant::now());
            status.total_messages += 1;
        }
    }

    pub async fn increment_reconnect_attempts(&self, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(status) = connections.get_mut(connection_id) {
            status.reconnect_attempts += 1;
            warn!("Connection {} reconnect attempt {}/{}", 
                  connection_id, status.reconnect_attempts, self.config.max_reconnect_attempts);
        }
    }

    pub async fn get_connection_status(&self, connection_id: &str) -> Option<ConnectionStatus> {
        let connections = self.connections.read().await;
        connections.get(connection_id).cloned()
    }

    pub async fn get_all_connections(&self) -> Vec<ConnectionStatus> {
        let connections = self.connections.read().await;
        connections.values().cloned().collect()
    }

    pub async fn get_connection_summary(&self) -> ConnectionSummary {
        let connections = self.connections.read().await;
        let total_connections = connections.len();
        let connected_count = connections.values().filter(|c| c.is_connected).count();
        let total_messages: u64 = connections.values().map(|c| c.total_messages).sum();
        let total_errors: usize = connections.values().map(|c| c.errors.len()).sum();

        ConnectionSummary {
            total_connections,
            connected_count,
            disconnected_count: total_connections - connected_count,
            total_messages,
            total_errors,
        }
    }

    pub async fn cleanup_dead_connections(&self) {
        let mut connections = self.connections.write().await;
        let now = Instant::now();
        let timeout = Duration::from_secs(self.config.connection_timeout_seconds * 2);

        connections.retain(|_, status| {
            if let Some(last_message) = status.last_message {
                if now.duration_since(last_message) > timeout {
                    warn!("Removing dead connection {} (no messages for {:?})", 
                          status.connection_id, now.duration_since(last_message));
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionSummary {
    pub total_connections: usize,
    pub connected_count: usize,
    pub disconnected_count: usize,
    pub total_messages: u64,
    pub total_errors: usize,
}

impl ConnectionSummary {
    pub fn health_percentage(&self) -> f64 {
        if self.total_connections == 0 {
            100.0
        } else {
            (self.connected_count as f64 / self.total_connections as f64) * 100.0
        }
    }
} 