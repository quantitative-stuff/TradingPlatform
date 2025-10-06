use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ConnectionStats {
    pub connected: usize,      // Currently connected WebSockets
    pub disconnected: usize,   // Currently disconnected WebSockets (will retry)
    pub total_connections: usize,  // Total connection attempts
    pub reconnect_count: usize,     // Number of successful reconnections
    pub last_error: Option<String>,
}

pub static CONNECTION_STATS: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, ConnectionStats>>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));