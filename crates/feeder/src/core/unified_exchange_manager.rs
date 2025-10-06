/// Unified exchange connection manager that handles connection, reconnection, 
/// and lifecycle management for all exchanges in a consistent way
///
/// This ensures all exchanges follow the same patterns for:
/// - Initial connection with retries
/// - Subscription management
/// - Automatic reconnection on failure
/// - Health monitoring
/// - Graceful shutdown

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn, error, debug};
use anyhow::Result;

use super::{
    Feeder,
    RobustConnectionManager,
    ExchangeConnectionLimits,
    get_shutdown_receiver,
};

/// Configuration for unified connection management
#[derive(Debug, Clone)]
pub struct UnifiedConnectionConfig {
    /// Initial delay between reconnection attempts
    pub initial_reconnect_delay: Duration,
    /// Maximum delay between reconnection attempts
    pub max_reconnect_delay: Duration,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Maximum consecutive failures before giving up
    pub max_consecutive_failures: u32,
    /// Health check interval
    pub health_check_interval: Duration,
    /// Stale connection timeout (no messages received)
    pub stale_connection_timeout: Duration,
}

impl Default for UnifiedConnectionConfig {
    fn default() -> Self {
        Self {
            initial_reconnect_delay: Duration::from_secs(5),
            max_reconnect_delay: Duration::from_secs(300),
            backoff_multiplier: 1.5,
            max_consecutive_failures: 10,
            health_check_interval: Duration::from_secs(60),
            stale_connection_timeout: Duration::from_secs(300),
        }
    }
}

/// Unified exchange manager that wraps any exchange implementing the Feeder trait
pub struct UnifiedExchangeManager<E: Feeder> {
    exchange: E,
    config: UnifiedConnectionConfig,
    exchange_name: String,
}

impl<E: Feeder> UnifiedExchangeManager<E> {
    /// Create a new unified manager for an exchange
    pub fn new(exchange: E, config: Option<UnifiedConnectionConfig>) -> Self {
        let exchange_name = exchange.name().to_string();
        Self {
            exchange,
            config: config.unwrap_or_default(),
            exchange_name,
        }
    }
    
    /// Run the exchange with automatic connection management
    pub async fn run(&mut self) -> Result<()> {
        info!("{}: Starting unified connection manager", self.exchange_name);
        
        let mut consecutive_failures = 0;
        let mut reconnect_delay = self.config.initial_reconnect_delay;
        let mut shutdown_rx = get_shutdown_receiver();
        
        // Main reconnection loop
        loop {
            // Check for shutdown
            if *shutdown_rx.borrow() {
                info!("{}: Shutdown signal received, stopping", self.exchange_name);
                break;
            }
            
            // Connection phase
            info!("{}: Attempting connection (attempt {})", 
                self.exchange_name, consecutive_failures + 1);
            
            match self.connect_with_retry().await {
                Ok(_) => {
                    info!("{}: Connected successfully", self.exchange_name);
                    consecutive_failures = 0;
                    reconnect_delay = self.config.initial_reconnect_delay;
                    
                    // Subscription phase
                    match self.subscribe_with_retry().await {
                        Ok(_) => {
                            info!("{}: Subscribed successfully", self.exchange_name);
                            
                            // Start phase (runs until disconnection)
                            match self.exchange.start().await {
                                Ok(_) => {
                                    // start() returned normally - connections lost
                                    warn!("{}: Connections lost, will reconnect", self.exchange_name);
                                }
                                Err(e) => {
                                    error!("{}: Start error: {}", self.exchange_name, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("{}: Subscribe failed: {}", self.exchange_name, e);
                        }
                    }
                }
                Err(e) => {
                    consecutive_failures += 1;
                    error!("{}: Connection failed (attempt {}): {}", 
                        self.exchange_name, consecutive_failures, e);
                    
                    if consecutive_failures >= self.config.max_consecutive_failures {
                        error!("{}: Too many consecutive failures ({}), giving up", 
                            self.exchange_name, consecutive_failures);
                        return Err(anyhow::anyhow!("Max consecutive failures reached"));
                    }
                }
            }
            
            // Wait before reconnecting
            warn!("{}: Waiting {:?} before reconnection attempt", 
                self.exchange_name, reconnect_delay);
            
            tokio::select! {
                _ = sleep(reconnect_delay) => {
                    // Continue to next iteration
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("{}: Shutdown during reconnect delay", self.exchange_name);
                        break;
                    }
                }
            }
            
            // Exponential backoff
            reconnect_delay = std::cmp::min(
                Duration::from_secs_f64(reconnect_delay.as_secs_f64() * self.config.backoff_multiplier),
                self.config.max_reconnect_delay
            );
        }
        
        info!("{}: Unified connection manager stopped", self.exchange_name);
        Ok(())
    }
    
    /// Connect with retries
    async fn connect_with_retry(&mut self) -> Result<()> {
        let max_attempts = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.exchange.connect().await {
                Ok(_) => return Ok(()),
                Err(e) if attempt < max_attempts => {
                    warn!("{}: Connect attempt {} failed: {}, retrying...", 
                        self.exchange_name, attempt, e);
                    sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    
    /// Subscribe with retries
    async fn subscribe_with_retry(&mut self) -> Result<()> {
        let max_attempts = 3;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            match self.exchange.subscribe().await {
                Ok(_) => return Ok(()),
                Err(e) if attempt < max_attempts => {
                    warn!("{}: Subscribe attempt {} failed: {}, retrying...", 
                        self.exchange_name, attempt, e);
                    sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// Builder pattern for creating exchange managers with custom config
pub struct UnifiedExchangeBuilder {
    config: UnifiedConnectionConfig,
}

impl UnifiedExchangeBuilder {
    pub fn new() -> Self {
        Self {
            config: UnifiedConnectionConfig::default(),
        }
    }
    
    pub fn with_reconnect_delay(mut self, initial: Duration, max: Duration) -> Self {
        self.config.initial_reconnect_delay = initial;
        self.config.max_reconnect_delay = max;
        self
    }
    
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.config.backoff_multiplier = multiplier;
        self
    }
    
    pub fn with_max_failures(mut self, max: u32) -> Self {
        self.config.max_consecutive_failures = max;
        self
    }
    
    pub fn with_health_check(mut self, interval: Duration, timeout: Duration) -> Self {
        self.config.health_check_interval = interval;
        self.config.stale_connection_timeout = timeout;
        self
    }
    
    /// Build manager for specific exchange with its rate limits
    pub fn build_for<E: Feeder>(self, exchange: E) -> UnifiedExchangeManager<E> {
        let exchange_name = exchange.name();
        let limits = ExchangeConnectionLimits::for_exchange(exchange_name);
        
        // Override config with exchange-specific limits
        let mut config = self.config;
        config.initial_reconnect_delay = limits.initial_reconnect_delay;
        config.max_reconnect_delay = limits.max_reconnect_delay;
        config.backoff_multiplier = limits.backoff_multiplier;
        
        UnifiedExchangeManager {
            exchange,
            config,
            exchange_name: exchange_name.to_string(),
        }
    }
}

/// Helper macro to run any exchange with unified management
#[macro_export]
macro_rules! run_exchange_unified {
    ($exchange:expr) => {{
        let mut manager = UnifiedExchangeBuilder::new()
            .build_for($exchange);
        manager.run().await
    }};
    ($exchange:expr, $config:expr) => {{
        let mut manager = UnifiedExchangeManager::new($exchange, Some($config));
        manager.run().await
    }};
}