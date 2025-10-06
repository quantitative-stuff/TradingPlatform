use std::sync::Arc;
use chrono::Utc;
use std::collections::HashMap;
use tracing::{info, error};

use crate::connect_to_databse::{
    FeederLogger, ConnectionEvent, PerformanceMetrics, 
    ErrorLog, ProcessingStats, SystemMetricsCollector
};
use crate::error::Result;

/// Global feeder logger instance for operational logging
static FEEDER_LOGGER: once_cell::sync::OnceCell<Arc<FeederLogger>> = once_cell::sync::OnceCell::new();

/// Initialize the global feeder logger
pub async fn init_feeder_logger(logger: FeederLogger) -> Result<()> {
    FEEDER_LOGGER.set(Arc::new(logger))
        .map_err(|_| crate::error::Error::Config("Feeder logger already initialized".to_string()))?;
    
    info!("Feeder operational logger initialized");
    
    // Start performance metrics collector
    start_performance_collector().await;
    
    Ok(())
}

/// Get the global feeder logger
pub fn get_feeder_logger() -> Option<Arc<FeederLogger>> {
    FEEDER_LOGGER.get().cloned()
}

/// Log a connection event
pub async fn log_connection(exchange: &str, connection_id: &str, event: ConnectionEvent) -> Result<()> {
    if let Some(logger) = get_feeder_logger() {
        logger.log_connection_event(exchange, connection_id, event).await?;
    }
    Ok(())
}

/// Log performance metrics
pub async fn log_performance(metrics: PerformanceMetrics) -> Result<()> {
    if let Some(logger) = get_feeder_logger() {
        logger.log_performance(metrics).await?;
    }
    Ok(())
}

/// Log an error
pub async fn log_error(error: ErrorLog) -> Result<()> {
    if let Some(logger) = get_feeder_logger() {
        logger.log_error(error).await?;
    }
    Ok(())
}

/// Log processing statistics
pub async fn log_processing_stats(stats: ProcessingStats) -> Result<()> {
    if let Some(logger) = get_feeder_logger() {
        logger.log_processing_stats(stats).await?;
    }
    Ok(())
}

/// Start automatic performance metrics collection
async fn start_performance_collector() {
    tokio::spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        
        loop {
            interval.tick().await;
            
            // Collect metrics for each exchange
            for exchange in ["Binance", "Bybit", "Upbit"] {
                let metrics = SystemMetricsCollector::collect_performance_metrics(exchange);
                
                if let Err(e) = log_performance(metrics).await {
                    error!("Failed to log performance metrics: {}", e);
                }
            }
        }
    });
    
    info!("Performance metrics collector started (10s interval)");
}

/// Connection lifecycle hooks for WebSocket connections
pub struct ConnectionLifecycle;

impl ConnectionLifecycle {
    /// Called when a connection is established
    pub async fn on_connected(exchange: &str, connection_id: &str, symbols: Vec<String>) {
        let _ = log_connection(
            exchange,
            connection_id,
            ConnectionEvent::Connected,
        ).await;
        
        let _ = log_connection(
            exchange,
            connection_id,
            ConnectionEvent::SubscriptionSuccess { symbols },
        ).await;
    }
    
    /// Called when a connection is lost
    pub async fn on_disconnected(exchange: &str, connection_id: &str, reason: String) {
        let _ = log_connection(
            exchange,
            connection_id,
            ConnectionEvent::Disconnected { reason },
        ).await;
    }
    
    /// Called when reconnecting
    pub async fn on_reconnecting(exchange: &str, connection_id: &str, attempt: u32) {
        let _ = log_connection(
            exchange,
            connection_id,
            ConnectionEvent::Reconnecting { attempt },
        ).await;
    }
    
    /// Called when rate limited
    pub async fn on_rate_limited(exchange: &str, connection_id: &str, retry_after_ms: u64) {
        let _ = log_connection(
            exchange,
            connection_id,
            ConnectionEvent::RateLimited { retry_after_ms },
        ).await;
    }
    
    /// Called on error
    pub async fn on_error(exchange: &str, connection_id: &str, error: String) {
        let _ = log_connection(
            exchange,
            connection_id,
            ConnectionEvent::Error { error: error.clone() },
        ).await;
        
        // Also log as error
        let _ = log_error(ErrorLog {
            timestamp: Utc::now(),
            exchange: exchange.to_string(),
            component: "WebSocket".to_string(),
            error_type: "Connection".to_string(),
            message: error,
            context: HashMap::new(),
            stack_trace: None,
        }).await;
    }
}

/// Data processing hooks
pub struct DataProcessingHooks;

impl DataProcessingHooks {
    /// Track message processing statistics
    pub async fn update_stats(
        exchange: &str,
        data_type: &str,
        received: u64,
        processed: u64,
        dropped: u64,
        parsing_errors: u32,
        avg_time_us: f64,
    ) {
        let stats = ProcessingStats {
            timestamp: Utc::now(),
            exchange: exchange.to_string(),
            data_type: data_type.to_string(),
            messages_received: received,
            messages_processed: processed,
            messages_dropped: dropped,
            parsing_errors,
            avg_processing_time_us: avg_time_us,
        };
        
        let _ = log_processing_stats(stats).await;
    }
    
    /// Log parsing error
    pub async fn log_parsing_error(exchange: &str, data_type: &str, error: &str) {
        let error_log = ErrorLog {
            timestamp: Utc::now(),
            exchange: exchange.to_string(),
            component: "Parser".to_string(),
            error_type: "ParseError".to_string(),
            message: error.to_string(),
            context: {
                let mut ctx = HashMap::new();
                ctx.insert("data_type".to_string(), data_type.to_string());
                ctx
            },
            stack_trace: None,
        };
        
        let _ = log_error(error_log).await;
    }
}

/// Summary statistics for monitoring
pub struct FeederStatsSummary {
    pub total_connections: u32,
    pub active_connections: u32,
    pub total_messages: u64,
    pub errors_last_hour: u32,
    pub avg_latency_ms: f64,
    pub uptime_seconds: u64,
}

impl FeederStatsSummary {
    pub async fn collect() -> Self {
        // This would aggregate data from CONNECTION_STATS and other sources
        // For now, return placeholder
        Self {
            total_connections: 0,
            active_connections: 0,
            total_messages: 0,
            errors_last_hour: 0,
            avg_latency_ms: 0.0,
            uptime_seconds: 0,
        }
    }
    
    pub async fn log_summary(&self) {
        info!(
            "Feeder Stats - Connections: {}/{}, Messages: {}, Errors: {}, Latency: {:.2}ms, Uptime: {}s",
            self.active_connections,
            self.total_connections,
            self.total_messages,
            self.errors_last_hour,
            self.avg_latency_ms,
            self.uptime_seconds
        );
    }
}