use crate::error::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::error;

// Structured log entry for database storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub exchange: Option<String>,
    pub component: String,
    pub message: String,
    pub error_code: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    ERROR,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

impl LogLevel {
    pub fn as_str(&self) -> &str {
        match self {
            LogLevel::ERROR => "ERROR",
            LogLevel::WARN => "WARN",
            LogLevel::INFO => "INFO",
            LogLevel::DEBUG => "DEBUG",
            LogLevel::TRACE => "TRACE",
        }
    }
}

// Metrics that can be extracted from logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMetrics {
    pub timestamp: DateTime<Utc>,
    pub exchange: String,
    pub error_count: u64,
    pub warn_count: u64,
    pub avg_latency_ms: Option<f64>,
    pub connection_drops: u64,
    pub reconnect_attempts: u64,
    pub messages_processed: u64,
}

#[async_trait]
pub trait LogWriter: Send + Sync {
    async fn write_log(&mut self, entry: &LogEntry) -> Result<()>;
    async fn write_metrics(&mut self, metrics: &LogMetrics) -> Result<()>;
    async fn flush(&mut self) -> Result<()>;
}

// QuestDB implementation for log storage
pub struct QuestDBLogWriter {
    client: crate::connect_to_databse::QuestDBClient,
    tx: mpsc::UnboundedSender<LogEntry>,
}

impl QuestDBLogWriter {
    pub async fn new(client: crate::connect_to_databse::QuestDBClient) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<LogEntry>();
        
        // Spawn background task to batch and send logs
        let mut client_clone = client.clone();
        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(100);
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            
            loop {
                tokio::select! {
                    Some(entry) = rx.recv() => {
                        batch.push(entry);
                        if batch.len() >= 100 {
                            if let Err(e) = Self::send_batch(&mut client_clone, &batch).await {
                                error!("Failed to send log batch: {}", e);
                            }
                            batch.clear();
                        }
                    }
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            if let Err(e) = Self::send_batch(&mut client_clone, &batch).await {
                                error!("Failed to send log batch: {}", e);
                            }
                            batch.clear();
                        }
                    }
                }
            }
        });
        
        Self { client, tx }
    }

    async fn send_batch(client: &mut crate::connect_to_databse::QuestDBClient, batch: &[LogEntry]) -> Result<()> {
        for entry in batch {
            let line = format!(
                "logs,exchange={},component={},level={} message=\"{}\",error_code=\"{}\" {}",
                entry.exchange.as_ref().unwrap_or(&"unknown".to_string()),
                entry.component,
                entry.level.as_str(),
                entry.message.replace("\"", "\\\""),
                entry.error_code.as_ref().unwrap_or(&"".to_string()),
                entry.timestamp.timestamp_nanos()
            );
            
            // client.send_line(&line).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl LogWriter for QuestDBLogWriter {
    async fn write_log(&mut self, entry: &LogEntry) -> Result<()> {
        self.tx.send(entry.clone())
            .map_err(|e| crate::error::Error::Channel(format!("Failed to send log: {}", e)))?;
        Ok(())
    }

    async fn write_metrics(&mut self, metrics: &LogMetrics) -> Result<()> {
        let line = format!(
            "log_metrics,exchange={} error_count={},warn_count={},avg_latency_ms={},connection_drops={},reconnect_attempts={},messages_processed={} {}",
            metrics.exchange,
            metrics.error_count,
            metrics.warn_count,
            metrics.avg_latency_ms.unwrap_or(0.0),
            metrics.connection_drops,
            metrics.reconnect_attempts,
            metrics.messages_processed,
            metrics.timestamp.timestamp_nanos()
        );
        
        // self.client.send_line(&line).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        // QuestDB handles flushing internally
        Ok(())
    }
}

// Log aggregator that collects logs and computes metrics
pub struct LogAggregator {
    writer: Box<dyn LogWriter>,
    metrics_interval: tokio::time::Duration,
    current_metrics: HashMap<String, LogMetrics>,
}

impl LogAggregator {
    pub fn new(writer: Box<dyn LogWriter>) -> Self {
        Self {
            writer,
            metrics_interval: tokio::time::Duration::from_secs(60),
            current_metrics: HashMap::new(),
        }
    }

    pub async fn process_log(&mut self, entry: LogEntry) -> Result<()> {
        // Write raw log
        self.writer.write_log(&entry).await?;
        
        // Update metrics
        if let Some(exchange) = &entry.exchange {
            let metrics = self.current_metrics.entry(exchange.clone()).or_insert_with(|| {
                LogMetrics {
                    timestamp: Utc::now(),
                    exchange: exchange.clone(),
                    error_count: 0,
                    warn_count: 0,
                    avg_latency_ms: None,
                    connection_drops: 0,
                    reconnect_attempts: 0,
                    messages_processed: 0,
                }
            });
            
            match entry.level {
                LogLevel::ERROR => metrics.error_count += 1,
                LogLevel::WARN => metrics.warn_count += 1,
                _ => {}
            }
            
            // Extract specific metrics from metadata
            if let Some(latency) = entry.metadata.get("latency_ms") {
                if let Some(lat_val) = latency.as_f64() {
                    let current_avg = metrics.avg_latency_ms.unwrap_or(0.0);
                    metrics.avg_latency_ms = Some((current_avg + lat_val) / 2.0);
                }
            }
            
            if entry.message.contains("connection dropped") || entry.message.contains("disconnected") {
                metrics.connection_drops += 1;
            }
            
            if entry.message.contains("reconnect") || entry.message.contains("retrying") {
                metrics.reconnect_attempts += 1;
            }
        }
        
        Ok(())
    }

    pub async fn flush_metrics(&mut self) -> Result<()> {
        for (_, metrics) in self.current_metrics.drain() {
            self.writer.write_metrics(&metrics).await?;
        }
        self.writer.flush().await?;
        Ok(())
    }
}