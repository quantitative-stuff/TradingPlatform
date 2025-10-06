/// Robust WebSocket connection management with exchange-specific limits and reconnection strategies
/// 
/// This module implements exchange-aware connection pooling, rate limiting, and reconnection
/// logic based on the documented API limits for each exchange.

use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::time::sleep;
use tracing::{info, warn, debug};

/// Exchange-specific connection configuration based on documented API limits
#[derive(Debug, Clone)]
pub struct ExchangeConnectionLimits {
    /// Maximum symbols per WebSocket connection
    pub max_symbols_per_connection: usize,
    /// Maximum messages per second rate limit
    pub max_messages_per_second: u32,
    /// Maximum concurrent connections allowed
    pub max_concurrent_connections: u32,
    /// Initial reconnection delay
    pub initial_reconnect_delay: Duration,
    /// Maximum reconnection delay (exponential backoff cap)
    pub max_reconnect_delay: Duration,
    /// Connection attempt window for rate limiting
    pub connection_attempt_window: Duration,
    /// Maximum connection attempts per window
    pub max_attempts_per_window: u32,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to use jitter in reconnection delays
    pub use_jitter: bool,
}

impl ExchangeConnectionLimits {
    /// Get connection limits for a specific exchange based on API documentation
    pub fn for_exchange(exchange: &str) -> Self {
        match exchange.to_lowercase().as_str() {
            "binance" => Self {
                max_symbols_per_connection: 20,  // Balanced: 20 symbols max for fault tolerance (vs 1024 API limit)
                max_messages_per_second: 4,      // Conservative vs 5 limit
                max_concurrent_connections: 50,  // Much higher to support more connections
                initial_reconnect_delay: Duration::from_secs(1),
                max_reconnect_delay: Duration::from_secs(300), // 5 minutes max
                connection_attempt_window: Duration::from_secs(300), // 5 minutes
                max_attempts_per_window: 200,    // Conservative vs 300 limit
                backoff_multiplier: 1.5,
                use_jitter: true,
            },
            "bybit" => Self {
                max_symbols_per_connection: 20,  // Balanced: 20 symbols max for fault tolerance (vs 200 topics API limit)
                max_messages_per_second: 15,     // Conservative vs 20 limit
                max_concurrent_connections: 100, // High capacity - Bybit supports 500 connections per 5 min
                initial_reconnect_delay: Duration::from_secs(1),
                max_reconnect_delay: Duration::from_secs(600), // 10 minutes max
                connection_attempt_window: Duration::from_secs(300), // 5 minute window
                max_attempts_per_window: 100,    // Generous within 500/5min limit
                backoff_multiplier: 1.5,
                use_jitter: true,
            },
            "okx" => Self {
                max_symbols_per_connection: 20,  // Balanced: 20 symbols max for fault tolerance (vs 100 subscriptions API limit)
                max_messages_per_second: 8,      // Conservative vs 10 limit
                max_concurrent_connections: 30,  // Higher to support more connections needed
                initial_reconnect_delay: Duration::from_secs(1),
                max_reconnect_delay: Duration::from_secs(300),
                connection_attempt_window: Duration::from_secs(120), // 2 minute window
                max_attempts_per_window: 6,      // Conservative vs 3 req/sec limit
                backoff_multiplier: 1.8,
                use_jitter: true,
            },
            "coinbase" => Self {
                max_symbols_per_connection: 8,   // Balanced: Close to 10 API limit but with buffer for fault tolerance
                max_messages_per_second: 6,      // Conservative vs 8 limit  
                max_concurrent_connections: 70,  // Higher to support more connections vs 75 limit
                initial_reconnect_delay: Duration::from_secs(1),
                max_reconnect_delay: Duration::from_secs(300),
                connection_attempt_window: Duration::from_secs(60),
                max_attempts_per_window: 20,
                backoff_multiplier: 2.0,
                use_jitter: true,
            },
            "deribit" => Self {
                max_symbols_per_connection: 20,  // Balanced: 20 symbols max for fault tolerance (vs 50 practical limit)
                max_messages_per_second: 8,      // Conservative, 10/min WebSocket limit
                max_concurrent_connections: 15,  // Higher to support more connections needed
                initial_reconnect_delay: Duration::from_secs(2),
                max_reconnect_delay: Duration::from_secs(600),
                connection_attempt_window: Duration::from_secs(60),
                max_attempts_per_window: 8,      // Conservative for 10/min limit
                backoff_multiplier: 2.0,         // Less aggressive backoff
                use_jitter: true,
            },
            "upbit" => Self {
                max_symbols_per_connection: 20,  // Balanced: 20 symbols max for fault tolerance (vs 100+ API limit)
                max_messages_per_second: 4,      // Conservative vs 5 message/sec limit  
                max_concurrent_connections: 50,  // Reasonable limit for more connections needed
                initial_reconnect_delay: Duration::from_secs(1), // Faster delay
                max_reconnect_delay: Duration::from_secs(300),   // Standard max delay
                connection_attempt_window: Duration::from_secs(300), // 5 minute window for rate limits
                max_attempts_per_window: 5,      // Standard attempts to match Upbit's 5 connections/sec
                backoff_multiplier: 2.0,         // Standard backoff
                use_jitter: true,
            },
            "bithumb" => Self {
                max_symbols_per_connection: 20,  // Aggressive: 20 symbols for fault tolerance vs 100 limit
                max_messages_per_second: 8,      // Conservative vs 10 limit
                max_concurrent_connections: 20,  // Increase connections
                initial_reconnect_delay: Duration::from_secs(1),
                max_reconnect_delay: Duration::from_secs(600),
                connection_attempt_window: Duration::from_secs(60),
                max_attempts_per_window: 20,
                backoff_multiplier: 1.8,
                use_jitter: true,
            },
            _ => {
                warn!("Unknown exchange '{}', using default conservative limits", exchange);
                Self::default()
            }
        }
    }
}

impl Default for ExchangeConnectionLimits {
    fn default() -> Self {
        Self {
            max_symbols_per_connection: 20,     // Very conservative default
            max_messages_per_second: 5,
            max_concurrent_connections: 5,
            initial_reconnect_delay: Duration::from_secs(5),
            max_reconnect_delay: Duration::from_secs(600),
            connection_attempt_window: Duration::from_secs(300),
            max_attempts_per_window: 10,
            backoff_multiplier: 2.0,
            use_jitter: true,
        }
    }
}

/// Connection attempt tracker for rate limiting
#[derive(Debug)]
struct ConnectionAttemptTracker {
    attempts: Vec<Instant>,
    limits: ExchangeConnectionLimits,
}

impl ConnectionAttemptTracker {
    fn new(limits: ExchangeConnectionLimits) -> Self {
        Self {
            attempts: Vec::new(),
            limits,
        }
    }

    /// Check if we can make a connection attempt, respecting rate limits
    fn can_attempt(&mut self) -> bool {
        let now = Instant::now();
        
        // Remove old attempts outside the window
        self.attempts.retain(|&attempt_time| {
            now.duration_since(attempt_time) < self.limits.connection_attempt_window
        });
        
        // Check if we're under the limit
        self.attempts.len() < self.limits.max_attempts_per_window as usize
    }

    /// Record a connection attempt
    fn record_attempt(&mut self) {
        self.attempts.push(Instant::now());
    }

    /// Get time until next attempt is allowed
    fn time_until_next_attempt(&self) -> Option<Duration> {
        if self.attempts.len() < self.limits.max_attempts_per_window as usize {
            return None;
        }

        // Find the oldest attempt that needs to expire
        if let Some(&oldest) = self.attempts.first() {
            let elapsed = Instant::now().duration_since(oldest);
            if elapsed < self.limits.connection_attempt_window {
                Some(self.limits.connection_attempt_window - elapsed)
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Robust connection manager with exchange-specific handling
pub struct RobustConnectionManager {
    exchange_limits: HashMap<String, ExchangeConnectionLimits>,
    attempt_trackers: HashMap<String, ConnectionAttemptTracker>,
    active_connections: HashMap<String, u32>,
}

impl RobustConnectionManager {
    pub fn new() -> Self {
        Self {
            exchange_limits: HashMap::new(),
            attempt_trackers: HashMap::new(),
            active_connections: HashMap::new(),
        }
    }

    /// Initialize limits for an exchange
    pub fn init_exchange(&mut self, exchange: &str) {
        let limits = ExchangeConnectionLimits::for_exchange(exchange);
        info!("Initializing {} with limits: {} symbols/conn, {} msg/sec, {} max connections", 
            exchange, limits.max_symbols_per_connection, limits.max_messages_per_second, limits.max_concurrent_connections);
        
        let tracker = ConnectionAttemptTracker::new(limits.clone());
        
        self.exchange_limits.insert(exchange.to_string(), limits);
        self.attempt_trackers.insert(exchange.to_string(), tracker);
        self.active_connections.insert(exchange.to_string(), 0);
    }

    /// Calculate optimal number of connections needed for symbols
    pub fn calculate_optimal_connections(&self, exchange: &str, total_symbols: usize) -> usize {
        let default_limits = ExchangeConnectionLimits::default();
        let limits = self.exchange_limits.get(exchange).unwrap_or(&default_limits);
        
        let connections_needed = (total_symbols + limits.max_symbols_per_connection - 1) / limits.max_symbols_per_connection;
        let max_allowed = limits.max_concurrent_connections as usize;
        
        let optimal = connections_needed.min(max_allowed);
        
        if connections_needed > max_allowed {
            warn!("{}: Need {} connections for {} symbols, but limited to {} connections. Some symbols may not be subscribed.", 
                exchange, connections_needed, total_symbols, max_allowed);
        }
        
        debug!("{}: Optimal connections: {} for {} symbols ({} per connection)", 
            exchange, optimal, total_symbols, limits.max_symbols_per_connection);
        
        optimal
    }

    /// Get symbols per connection for an exchange
    pub fn get_symbols_per_connection(&self, exchange: &str) -> usize {
        self.exchange_limits.get(exchange)
            .map(|l| l.max_symbols_per_connection)
            .unwrap_or(20)
    }

    /// Check if we can attempt a connection
    pub async fn can_connect(&mut self, exchange: &str) -> Result<(), Duration> {
        let tracker = self.attempt_trackers.get_mut(exchange).ok_or(Duration::from_secs(60))?;
        
        if tracker.can_attempt() {
            Ok(())
        } else {
            let wait_time = tracker.time_until_next_attempt().unwrap_or(Duration::from_secs(60));
            warn!("{}: Rate limited, must wait {:?} before next connection attempt", exchange, wait_time);
            Err(wait_time)
        }
    }

    /// Record a connection attempt
    pub fn record_connection_attempt(&mut self, exchange: &str) {
        if let Some(tracker) = self.attempt_trackers.get_mut(exchange) {
            tracker.record_attempt();
        }
        
        if let Some(count) = self.active_connections.get_mut(exchange) {
            *count += 1;
        }
    }

    /// Calculate reconnection delay with exponential backoff and jitter
    pub async fn wait_for_reconnect(&self, exchange: &str, attempt: u32) -> Duration {
        let default_limits = ExchangeConnectionLimits::default();
        let limits = self.exchange_limits.get(exchange).unwrap_or(&default_limits);
        
        // Exponential backoff calculation
        let base_delay = limits.initial_reconnect_delay;
        let multiplier = limits.backoff_multiplier.powi(attempt.saturating_sub(1) as i32);
        let calculated_delay = Duration::from_secs_f64(base_delay.as_secs_f64() * multiplier);
        
        // Cap at maximum delay
        let capped_delay = calculated_delay.min(limits.max_reconnect_delay);
        
        // Add jitter if enabled (±25% random variation)
        let final_delay = if limits.use_jitter {
            let jitter_factor = 0.75 + (rand::random::<f64>() * 0.5); // 0.75 to 1.25
            Duration::from_secs_f64(capped_delay.as_secs_f64() * jitter_factor)
        } else {
            capped_delay
        };

        info!("{}: Reconnection attempt {} - waiting {:?} before retry", 
            exchange, attempt, final_delay);
        
        sleep(final_delay).await;
        final_delay
    }

    /// Record connection success
    pub fn record_connection_success(&mut self, exchange: &str) {
        debug!("{}: Connection successful", exchange);
    }

    /// Record connection failure
    pub fn record_connection_failure(&mut self, exchange: &str) {
        if let Some(count) = self.active_connections.get_mut(exchange) {
            *count = count.saturating_sub(1);
        }
        debug!("{}: Connection failed", exchange);
    }

    /// Get active connection count
    pub fn get_active_connections(&self, exchange: &str) -> u32 {
        self.active_connections.get(exchange).copied().unwrap_or(0)
    }

    /// Check if we're under the concurrent connection limit
    pub fn can_add_connection(&self, exchange: &str) -> bool {
        let active = self.get_active_connections(exchange);
        let default_limits = ExchangeConnectionLimits::default();
        let limits = self.exchange_limits.get(exchange).unwrap_or(&default_limits);
        
        active < limits.max_concurrent_connections
    }
}