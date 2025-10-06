use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::error::Result;
use super::QuestDBClient;

/// Connection event types for tracking WebSocket lifecycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionEvent {
    Connected,
    Disconnected { reason: String },
    Reconnecting { attempt: u32 },
    Replaced { reason: String }, // Connection replaced due to rate limits or errors
    RateLimited { retry_after_ms: u64 },
    Error { error: String },
    SubscriptionSuccess { symbols: Vec<String> },
    SubscriptionFailure { error: String },
    // Stream lifecycle tracking events
    StreamCreated { symbols_count: i32 },
    StreamTaken { elapsed_ms: u64 },
    StreamStarted { symbols_count: i32 },
    StreamDropped { status: String, elapsed_ms: u64, symbols_count: i32 },
}

/// Performance metrics collected at regular intervals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub timestamp: DateTime<Utc>,
    pub exchange: String,
    pub active_connections: u32,
    pub messages_per_second: f64,
    pub avg_latency_ms: f64,
    pub memory_usage_mb: f64,
    pub cpu_usage_percent: f64,
    pub dropped_messages: u64,
    pub queue_depth: u32,
}

/// Connection statistics for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStats {
    pub exchange: String,
    pub connection_id: String,
    pub connected_at: DateTime<Utc>,
    pub disconnected_at: Option<DateTime<Utc>>,
    pub total_messages: u64,
    pub total_errors: u32,
    pub reconnect_count: u32,
    pub symbols: Vec<String>,
    pub asset_type: String,
}

/// Error log entry for debugging and analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLog {
    pub timestamp: DateTime<Utc>,
    pub exchange: String,
    pub component: String,
    pub error_type: String,
    pub message: String,
    pub context: HashMap<String, String>,
    pub stack_trace: Option<String>,
}

/// Data processing statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingStats {
    pub timestamp: DateTime<Utc>,
    pub exchange: String,
    pub data_type: String, // "trade", "orderbook", "ticker"
    pub messages_received: u64,
    pub messages_processed: u64,
    pub messages_dropped: u64,
    pub parsing_errors: u32,
    pub avg_processing_time_us: f64,
}

/// Connection event with metadata
struct ConnectionEventEntry {
    exchange: String,
    connection_id: String,
    event: ConnectionEvent,
}

/// Main feeder logger that handles all operational logging
#[derive(Clone)]
pub struct FeederLogger {
    questdb: Arc<RwLock<QuestDBClient>>,
    buffer_connection_events: Arc<RwLock<Vec<ConnectionEventEntry>>>,
    buffer_performance: Arc<RwLock<Vec<PerformanceMetrics>>>,
    buffer_errors: Arc<RwLock<Vec<ErrorLog>>>,
    buffer_processing: Arc<RwLock<Vec<ProcessingStats>>>,
    flush_interval_ms: u64,
}

impl FeederLogger {
    pub async fn new(
        questdb: QuestDBClient,
        flush_interval_ms: u64,
    ) -> Result<Self> {
        let logger = Self {
            questdb: Arc::new(RwLock::new(questdb)),
            buffer_connection_events: Arc::new(RwLock::new(Vec::new())),
            buffer_performance: Arc::new(RwLock::new(Vec::new())),
            buffer_errors: Arc::new(RwLock::new(Vec::new())),
            buffer_processing: Arc::new(RwLock::new(Vec::new())),
            flush_interval_ms,
        };

        // Start auto-flush task
        logger.start_auto_flush().await;

        Ok(logger)
    }

    /// Log a connection event
    pub async fn log_connection_event(
        &self,
        exchange: &str,
        connection_id: &str,
        event: ConnectionEvent,
    ) -> Result<()> {
        let mut buffer = self.buffer_connection_events.write().await;
        
        // Create log entry based on event type
        let log_message = match &event {
            ConnectionEvent::Connected => {
                info!("[{}] Connection {} established", exchange, connection_id);
                format!("Connection established: {}", connection_id)
            }
            ConnectionEvent::Disconnected { reason } => {
                warn!("[{}] Connection {} disconnected: {}", exchange, connection_id, reason);
                format!("Connection lost: {} - {}", connection_id, reason)
            }
            ConnectionEvent::Reconnecting { attempt } => {
                info!("[{}] Connection {} reconnecting (attempt {})", exchange, connection_id, attempt);
                format!("Reconnecting: {} (attempt {})", connection_id, attempt)
            }
            ConnectionEvent::Replaced { reason } => {
                warn!("[{}] Connection {} replaced: {}", exchange, connection_id, reason);
                format!("Connection replaced: {} - {}", connection_id, reason)
            }
            ConnectionEvent::RateLimited { retry_after_ms } => {
                warn!("[{}] Rate limited, retry after {}ms", exchange, retry_after_ms);
                format!("Rate limited: retry after {}ms", retry_after_ms)
            }
            ConnectionEvent::Error { error } => {
                error!("[{}] Connection error: {}", exchange, error);
                format!("Error: {}", error)
            }
            ConnectionEvent::SubscriptionSuccess { symbols } => {
                info!("[{}] Subscribed to {} symbols", exchange, symbols.len());
                format!("Subscribed: {} symbols", symbols.len())
            }
            ConnectionEvent::SubscriptionFailure { error } => {
                error!("[{}] Subscription failed: {}", exchange, error);
                format!("Subscription failed: {}", error)
            }
            ConnectionEvent::StreamCreated { symbols_count } => {
                info!("[{}] Stream {} created with {} symbols", exchange, connection_id, symbols_count);
                format!("Stream created: {} symbols", symbols_count)
            }
            ConnectionEvent::StreamTaken { elapsed_ms } => {
                info!("[{}] Stream {} taken after {}ms", exchange, connection_id, elapsed_ms);
                format!("Stream taken: {}ms", elapsed_ms)
            }
            ConnectionEvent::StreamStarted { symbols_count } => {
                info!("[{}] Stream {} started processing {} symbols", exchange, connection_id, symbols_count);
                format!("Stream started: {} symbols", symbols_count)
            }
            ConnectionEvent::StreamDropped { status, elapsed_ms, symbols_count } => {
                info!("[{}] Stream {} dropped (status: {}, lived: {}ms, symbols: {})", 
                      exchange, connection_id, status, elapsed_ms, symbols_count);
                format!("Stream dropped: status={}, lived={}ms, symbols={}", status, elapsed_ms, symbols_count)
            }
        };

        // Store event with metadata for batch processing
        buffer.push(ConnectionEventEntry {
            exchange: exchange.to_string(),
            connection_id: connection_id.to_string(),
            event,
        });

        // Flush connection events immediately to ensure they appear in QuestDB
        // This is important for debugging connection issues
        drop(buffer); // Release the lock before flushing
        self.flush_connection_events().await?;

        Ok(())
    }

    /// Log performance metrics
    pub async fn log_performance(&self, metrics: PerformanceMetrics) -> Result<()> {
        let mut buffer = self.buffer_performance.write().await;
        buffer.push(metrics);

        if buffer.len() >= 50 {
            self.flush_performance_metrics().await?;
        }

        Ok(())
    }

    /// Log an error
    pub async fn log_error(&self, error: ErrorLog) -> Result<()> {
        error!(
            "[{}] {} error: {}",
            error.exchange, error.component, error.message
        );

        let mut buffer = self.buffer_errors.write().await;
        buffer.push(error);

        if buffer.len() >= 50 {
            self.flush_errors().await?;
        }

        Ok(())
    }

    /// Log data processing statistics
    pub async fn log_processing_stats(&self, stats: ProcessingStats) -> Result<()> {
        let mut buffer = self.buffer_processing.write().await;
        buffer.push(stats);

        if buffer.len() >= 100 {
            self.flush_processing_stats().await?;
        }

        Ok(())
    }

    /// Flush connection events to QuestDB
    async fn flush_connection_events(&self) -> Result<()> {
        let mut buffer = self.buffer_connection_events.write().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let questdb = self.questdb.write().await;
        
        // Write each event to QuestDB
        info!("Flushing {} connection events to QuestDB", buffer.len());
        for entry in buffer.iter() {
            let event_desc = match &entry.event {
                ConnectionEvent::Connected => "connected".to_string(),
                ConnectionEvent::Disconnected { .. } => "disconnected".to_string(), 
                ConnectionEvent::Reconnecting { .. } => "reconnecting".to_string(),
                ConnectionEvent::Replaced { .. } => "replaced".to_string(),
                ConnectionEvent::RateLimited { .. } => "rate_limited".to_string(),
                ConnectionEvent::Error { .. } => "error".to_string(),
                ConnectionEvent::SubscriptionSuccess { symbols } => format!("subscription_success_{}_symbols", symbols.len()),
                ConnectionEvent::SubscriptionFailure { .. } => "subscription_failure".to_string(),
                ConnectionEvent::StreamCreated { symbols_count } => format!("stream_created_{}_symbols", symbols_count),
                ConnectionEvent::StreamTaken { elapsed_ms } => format!("stream_taken_{}ms", elapsed_ms),
                ConnectionEvent::StreamStarted { symbols_count } => format!("stream_started_{}_symbols", symbols_count),
                ConnectionEvent::StreamDropped { status, elapsed_ms, symbols_count } => format!("stream_dropped_{}_{}ms_{}_symbols", status, elapsed_ms, symbols_count),
            };
            info!("Logging to QuestDB: {} {} {} {:?}", 
                entry.exchange, entry.connection_id, event_desc, &entry.event);
            match &entry.event {
                ConnectionEvent::Connected => {
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "connected",
                        "success",
                        None,
                        Some(1)
                    ).await?;
                },
                ConnectionEvent::Disconnected { reason } => {
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "disconnected",
                        "error",
                        Some(reason.as_str()),
                        None
                    ).await?;
                },
                ConnectionEvent::Reconnecting { attempt } => {
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "reconnecting",
                        "pending",
                        None,
                        Some(*attempt)
                    ).await?;
                },
                ConnectionEvent::Replaced { reason } => {
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "replaced",
                        "warning",
                        Some(reason.as_str()),
                        None
                    ).await?;
                },
                ConnectionEvent::RateLimited { retry_after_ms } => {
                    let msg = format!("retry_after_{}ms", retry_after_ms);
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "rate_limited",
                        "warning",
                        Some(&msg),
                        None
                    ).await?;
                },
                ConnectionEvent::Error { error } => {
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "error",
                        "error",
                        Some(error.as_str()),
                        None
                    ).await?;
                },
                ConnectionEvent::SubscriptionSuccess { symbols } => {
                    let msg = format!("{}_symbols", symbols.len());
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "subscription",
                        "success",
                        Some(&msg),
                        None
                    ).await?;
                },
                ConnectionEvent::SubscriptionFailure { error } => {
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "subscription",
                        "error",
                        Some(error.as_str()),
                        None
                    ).await?;
                },
                ConnectionEvent::StreamCreated { symbols_count } => {
                    let msg = format!("stream_created_with_{}_symbols", symbols_count);
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "stream_created",
                        "success",
                        Some(&msg),
                        Some(*symbols_count as u32)
                    ).await?;
                },
                ConnectionEvent::StreamTaken { elapsed_ms } => {
                    let msg = format!("stream_taken_after_{}ms", elapsed_ms);
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "stream_taken",
                        "success",
                        Some(&msg),
                        None
                    ).await?;
                },
                ConnectionEvent::StreamStarted { symbols_count } => {
                    let msg = format!("stream_started_processing_{}_symbols", symbols_count);
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "stream_started",
                        "success",
                        Some(&msg),
                        Some(*symbols_count as u32)
                    ).await?;
                },
                ConnectionEvent::StreamDropped { status, elapsed_ms, symbols_count } => {
                    let msg = format!("stream_dropped_status_{}_lived_{}ms_symbols_{}", status, elapsed_ms, symbols_count);
                    questdb.write_connection_log_with_attempt(
                        &entry.exchange,
                        &entry.connection_id,
                        "stream_dropped",
                        "success",
                        Some(&msg),
                        Some(*symbols_count as u32)
                    ).await?;
                },
            };
        }
        
        // Flush QuestDB buffer
        questdb.flush().await?;
        
        buffer.clear();
        Ok(())
    }

    /// Flush performance metrics to QuestDB
    async fn flush_performance_metrics(&self) -> Result<()> {
        let mut buffer = self.buffer_performance.write().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let questdb = self.questdb.write().await;
        
        // Write each performance metric to QuestDB
        for metric in buffer.iter() {
            questdb.write_performance_log(
                &metric.exchange,
                metric.cpu_usage_percent,
                metric.memory_usage_mb,
                metric.active_connections,
                metric.messages_per_second,
                metric.avg_latency_ms,
            ).await?;
        }
        
        // Flush QuestDB buffer
        questdb.flush().await?;
        
        buffer.clear();
        Ok(())
    }

    /// Flush errors to QuestDB
    async fn flush_errors(&self) -> Result<()> {
        let mut buffer = self.buffer_errors.write().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let questdb = self.questdb.write().await;
        
        // Write each error to QuestDB
        for error in buffer.iter() {
            let context_str = serde_json::to_string(&error.context)
                .unwrap_or_else(|_| "{}".to_string());
            
            questdb.write_error_log(
                &error.error_type,
                &error.component,
                Some(&error.error_type),
                &error.message,
                Some(&context_str),
            ).await?;
        }
        
        // Flush QuestDB buffer
        questdb.flush().await?;
        
        buffer.clear();
        Ok(())
    }

    /// Flush processing stats to QuestDB
    async fn flush_processing_stats(&self) -> Result<()> {
        let mut buffer = self.buffer_processing.write().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let questdb = self.questdb.write().await;
        
        // Write each stat to QuestDB
        for stat in buffer.iter() {
            questdb.write_stats_log(
                &stat.exchange,
                &stat.data_type,
                stat.messages_received,
                stat.messages_processed,
                stat.messages_dropped,
            ).await?;
        }
        
        // Flush QuestDB buffer
        questdb.flush().await?;
        
        buffer.clear();
        Ok(())
    }

    /// Start auto-flush task
    async fn start_auto_flush(&self) {
        let flush_interval = std::time::Duration::from_millis(self.flush_interval_ms);
        
        let logger = self.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(flush_interval).await;
                
                // Flush all buffers periodically
                if let Err(e) = logger.flush_all().await {
                    error!("Failed to auto-flush feeder logs: {}", e);
                }
            }
        });
    }
    
    /// Flush all buffers
    pub async fn flush_all(&self) -> Result<()> {
        self.flush_connection_events().await?;
        self.flush_performance_metrics().await?;
        self.flush_errors().await?;
        self.flush_processing_stats().await?;
        info!("Auto-flush completed for all feeder logs");
        Ok(())
    }

    /// Get current statistics for monitoring
    pub async fn get_current_stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        
        // Gather current buffer sizes
        stats.insert(
            "connection_events_buffered".to_string(),
            serde_json::json!(self.buffer_connection_events.read().await.len()),
        );
        stats.insert(
            "performance_metrics_buffered".to_string(),
            serde_json::json!(self.buffer_performance.read().await.len()),
        );
        stats.insert(
            "errors_buffered".to_string(),
            serde_json::json!(self.buffer_errors.read().await.len()),
        );
        stats.insert(
            "processing_stats_buffered".to_string(),
            serde_json::json!(self.buffer_processing.read().await.len()),
        );

        stats
    }
}

/// Helper to collect system metrics
pub struct SystemMetricsCollector;

impl SystemMetricsCollector {
    pub fn collect_performance_metrics(exchange: &str) -> PerformanceMetrics {
        use sysinfo::System;
        use crate::core::CONNECTION_STATS;
        
        let mut sys = System::new_all();
        sys.refresh_all();
        
        // Get real memory and CPU usage
        let memory_mb = sys.used_memory() as f64 / 1024.0; // KB to MB
        let cpu_percent = sys.global_cpu_info().cpu_usage() as f64;
        
        // Get real connection stats from the global HashMap
        let stats = CONNECTION_STATS.read();
        let exchange_lower = exchange.to_lowercase();
        
        // Get stats for this exchange
        let active_connections = if let Some(exchange_stats) = stats.get(&exchange_lower) {
            exchange_stats.connected as u32
        } else {
            0
        };
        
        // Get total connections for this exchange
        let total_connections = if let Some(exchange_stats) = stats.get(&exchange_lower) {
            exchange_stats.total_connections as u64
        } else {
            0
        };
        
        // Simple rate calculation (would need time tracking for accurate rate)
        let messages_per_second = if active_connections > 0 {
            // Estimate based on typical WebSocket message rates
            active_connections as f64 * 2.5 // Rough estimate: 2-3 messages per second per connection
        } else {
            0.0
        };
        
        PerformanceMetrics {
            timestamp: Utc::now(),
            exchange: exchange.to_string(),
            active_connections,
            messages_per_second,
            avg_latency_ms: if active_connections > 0 { 15.0 } else { 0.0 }, // Typical WebSocket latency
            memory_usage_mb: memory_mb,
            cpu_usage_percent: cpu_percent,
            dropped_messages: 0, // Could track from actual drops
            queue_depth: total_connections.saturating_sub(active_connections as u64) as u32, // Pending connections
        }
    }
}