/// Monitoring and metrics for orderbook builder
///
/// Provides real-time metrics, health checks, and performance monitoring

use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error};
use rand::Rng;

/// Metrics for orderbook builder
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderBookMetrics {
    // Throughput metrics
    pub updates_per_second: f64,
    pub snapshots_received: u64,
    pub total_updates_processed: u64,
    pub total_bytes_processed: u64,

    // Latency metrics (microseconds)
    pub avg_update_latency: u64,
    pub p50_latency: u64,
    pub p95_latency: u64,
    pub p99_latency: u64,
    pub max_latency: u64,

    // Gap detection metrics
    pub gaps_detected: u64,
    pub missing_updates: u64,
    pub resyncs_triggered: u64,
    pub successful_recoveries: u64,
    pub failed_recoveries: u64,

    // Orderbook quality metrics
    pub crossed_book_events: u64,
    pub invalid_price_levels: u64,
    pub checksum_failures: u64,
    pub stale_data_warnings: u64,

    // Resource utilization
    pub buffer_utilization: f64,
    pub memory_usage_mb: f64,
    pub cpu_usage_percent: f64,

    // Connection metrics
    pub connection_drops: u64,
    pub reconnect_attempts: u64,
    pub uptime_seconds: u64,
    pub last_update_timestamp: Option<i64>,
}

/// Health status of the orderbook builder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

/// Health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub status: HealthStatus,
    pub orderbook_synchronized: bool,
    pub last_update_age_ms: u64,
    pub gap_rate_per_hour: f64,
    pub error_rate_per_hour: f64,
    pub checks: Vec<HealthCheckItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckItem {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Latency tracker using reservoir sampling
pub struct LatencyTracker {
    samples: Vec<u64>,
    max_samples: usize,
    total_samples: u64,
    sum: u64,
}

impl LatencyTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Vec::with_capacity(max_samples),
            max_samples,
            total_samples: 0,
            sum: 0,
        }
    }

    pub fn record(&mut self, latency_us: u64) {
        self.total_samples += 1;
        self.sum += latency_us;

        if self.samples.len() < self.max_samples {
            self.samples.push(latency_us);
        } else {
            // Reservoir sampling for even distribution
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..self.total_samples as usize);
            if idx < self.max_samples {
                self.samples[idx] = latency_us;
            }
        }
    }

    pub fn percentile(&mut self, p: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }

        self.samples.sort_unstable();
        let idx = ((self.samples.len() - 1) as f64 * p / 100.0) as usize;
        self.samples[idx]
    }

    pub fn average(&self) -> u64 {
        if self.total_samples == 0 {
            0
        } else {
            self.sum / self.total_samples
        }
    }

    pub fn max(&self) -> u64 {
        *self.samples.iter().max().unwrap_or(&0)
    }
}

/// Monitoring system for orderbook builder
#[derive(Clone)]
pub struct MonitoringSystem {
    metrics: Arc<RwLock<OrderBookMetrics>>,
    latency_tracker: Arc<RwLock<LatencyTracker>>,
    start_time: Arc<RwLock<Instant>>,
    last_gap_time: Arc<RwLock<Option<Instant>>>,
    last_error_time: Arc<RwLock<Option<Instant>>>,
    symbol: Arc<String>,
    exchange: Arc<String>,
}

impl MonitoringSystem {
    pub fn new(symbol: String, exchange: String) -> Self {
        Self {
            metrics: Arc::new(RwLock::new(OrderBookMetrics::default())),
            latency_tracker: Arc::new(RwLock::new(LatencyTracker::new(10000))),
            start_time: Arc::new(RwLock::new(Instant::now())),
            last_gap_time: Arc::new(RwLock::new(None)),
            last_error_time: Arc::new(RwLock::new(None)),
            symbol: Arc::new(symbol),
            exchange: Arc::new(exchange),
        }
    }

    /// Record update processing
    pub fn record_update(&self, latency_us: u64, bytes: u64) {
        let mut metrics = self.metrics.write();
        metrics.total_updates_processed += 1;
        metrics.total_bytes_processed += bytes;
        metrics.last_update_timestamp = Some(chrono::Utc::now().timestamp_millis());

        // Update throughput
        let elapsed = self.start_time.read().elapsed().as_secs_f64();
        if elapsed > 0.0 {
            metrics.updates_per_second = metrics.total_updates_processed as f64 / elapsed;
        }

        // Record latency
        let mut tracker = self.latency_tracker.write();
        tracker.record(latency_us);
    }

    /// Record snapshot received
    pub fn record_snapshot(&self) {
        let mut metrics = self.metrics.write();
        metrics.snapshots_received += 1;
    }

    /// Record gap detected
    pub fn record_gap(&self, missing_count: u64) {
        let mut metrics = self.metrics.write();
        metrics.gaps_detected += 1;
        metrics.missing_updates += missing_count;

        *self.last_gap_time.write() = Some(Instant::now());
    }

    /// Record resync
    pub fn record_resync(&self, success: bool) {
        let mut metrics = self.metrics.write();
        metrics.resyncs_triggered += 1;

        if success {
            metrics.successful_recoveries += 1;
        } else {
            metrics.failed_recoveries += 1;
            *self.last_error_time.write() = Some(Instant::now());
        }
    }

    /// Record orderbook validation error
    pub fn record_validation_error(&self, error_type: ValidationError) {
        let mut metrics = self.metrics.write();

        match error_type {
            ValidationError::CrossedBook => metrics.crossed_book_events += 1,
            ValidationError::InvalidPrice => metrics.invalid_price_levels += 1,
            ValidationError::ChecksumMismatch => metrics.checksum_failures += 1,
            ValidationError::StaleData => metrics.stale_data_warnings += 1,
        }

        *self.last_error_time.write() = Some(Instant::now());
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> OrderBookMetrics {
        let mut metrics = self.metrics.write().clone();

        // Update latency percentiles
        let mut tracker = self.latency_tracker.write();
        metrics.avg_update_latency = tracker.average();
        metrics.p50_latency = tracker.percentile(50.0);
        metrics.p95_latency = tracker.percentile(95.0);
        metrics.p99_latency = tracker.percentile(99.0);
        metrics.max_latency = tracker.max();

        // Update uptime
        metrics.uptime_seconds = self.start_time.read().elapsed().as_secs();

        metrics
    }

    /// Perform health check
    pub fn health_check(&self, max_update_age_ms: u64) -> HealthCheck {
        let metrics = self.get_metrics();
        let mut checks = Vec::new();
        let mut is_healthy = true;
        let mut is_degraded = false;

        // Check 1: Update recency
        let update_age_ms = if let Some(ts) = metrics.last_update_timestamp {
            (chrono::Utc::now().timestamp_millis() - ts) as u64
        } else {
            u64::MAX
        };

        checks.push(HealthCheckItem {
            name: "Update Recency".to_string(),
            passed: update_age_ms < max_update_age_ms,
            message: format!("Last update: {}ms ago", update_age_ms),
        });

        if update_age_ms > max_update_age_ms * 2 {
            is_healthy = false;
        } else if update_age_ms > max_update_age_ms {
            is_degraded = true;
        }

        // Check 2: Gap rate
        let gap_rate_per_hour = if metrics.uptime_seconds > 0 {
            (metrics.gaps_detected as f64 / metrics.uptime_seconds as f64) * 3600.0
        } else {
            0.0
        };

        checks.push(HealthCheckItem {
            name: "Gap Rate".to_string(),
            passed: gap_rate_per_hour < 10.0,
            message: format!("{:.2} gaps/hour", gap_rate_per_hour),
        });

        if gap_rate_per_hour > 100.0 {
            is_healthy = false;
        } else if gap_rate_per_hour > 10.0 {
            is_degraded = true;
        }

        // Check 3: Error rate
        let total_errors = metrics.crossed_book_events
            + metrics.invalid_price_levels
            + metrics.checksum_failures;

        let error_rate_per_hour = if metrics.uptime_seconds > 0 {
            (total_errors as f64 / metrics.uptime_seconds as f64) * 3600.0
        } else {
            0.0
        };

        checks.push(HealthCheckItem {
            name: "Error Rate".to_string(),
            passed: error_rate_per_hour < 5.0,
            message: format!("{:.2} errors/hour", error_rate_per_hour),
        });

        if error_rate_per_hour > 50.0 {
            is_healthy = false;
        } else if error_rate_per_hour > 5.0 {
            is_degraded = true;
        }

        // Check 4: Latency
        let latency_ok = metrics.p99_latency < 10000; // 10ms
        checks.push(HealthCheckItem {
            name: "Latency".to_string(),
            passed: latency_ok,
            message: format!("P99: {}μs", metrics.p99_latency),
        });

        if metrics.p99_latency > 50000 {
            is_degraded = true;
        }

        // Determine overall status
        let status = if !is_healthy {
            HealthStatus::Unhealthy {
                reason: format!("Failed health checks: {}",
                    checks.iter()
                        .filter(|c| !c.passed)
                        .map(|c| c.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        } else if is_degraded {
            HealthStatus::Degraded {
                reason: "Performance below optimal levels".to_string(),
            }
        } else {
            HealthStatus::Healthy
        };

        HealthCheck {
            status,
            orderbook_synchronized: update_age_ms < max_update_age_ms,
            last_update_age_ms: update_age_ms,
            gap_rate_per_hour,
            error_rate_per_hour,
            checks,
        }
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> String {
        let metrics = self.get_metrics();
        let mut output = String::new();

        // Add metric lines in Prometheus format
        output.push_str(&format!(
            "# HELP orderbook_updates_total Total number of orderbook updates processed\n\
             # TYPE orderbook_updates_total counter\n\
             orderbook_updates_total{{symbol=\"{}\",exchange=\"{}\"}} {}\n\n",
            self.symbol, self.exchange, metrics.total_updates_processed
        ));

        output.push_str(&format!(
            "# HELP orderbook_gaps_total Total number of sequence gaps detected\n\
             # TYPE orderbook_gaps_total counter\n\
             orderbook_gaps_total{{symbol=\"{}\",exchange=\"{}\"}} {}\n\n",
            self.symbol, self.exchange, metrics.gaps_detected
        ));

        output.push_str(&format!(
            "# HELP orderbook_latency_microseconds Update processing latency\n\
             # TYPE orderbook_latency_microseconds histogram\n\
             orderbook_latency_microseconds{{symbol=\"{}\",exchange=\"{}\",quantile=\"0.5\"}} {}\n\
             orderbook_latency_microseconds{{symbol=\"{}\",exchange=\"{}\",quantile=\"0.95\"}} {}\n\
             orderbook_latency_microseconds{{symbol=\"{}\",exchange=\"{}\",quantile=\"0.99\"}} {}\n\n",
            self.symbol, self.exchange, metrics.p50_latency,
            self.symbol, self.exchange, metrics.p95_latency,
            self.symbol, self.exchange, metrics.p99_latency
        ));

        output.push_str(&format!(
            "# HELP orderbook_throughput_updates_per_second Current update throughput\n\
             # TYPE orderbook_throughput_updates_per_second gauge\n\
             orderbook_throughput_updates_per_second{{symbol=\"{}\",exchange=\"{}\"}} {:.2}\n",
            self.symbol, self.exchange, metrics.updates_per_second
        ));

        output
    }
}

/// Validation error types
#[derive(Debug)]
pub enum ValidationError {
    CrossedBook,
    InvalidPrice,
    ChecksumMismatch,
    StaleData,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitoring_system() {
        let monitor = MonitoringSystem::new("BTCUSDT".to_string(), "Binance".to_string());

        // Record some updates
        for i in 0..100 {
            monitor.record_update(100 + i * 10, 1024);
        }

        // Record some events
        monitor.record_snapshot();
        monitor.record_gap(5);
        monitor.record_resync(true);

        // Get metrics
        let metrics = monitor.get_metrics();
        assert_eq!(metrics.total_updates_processed, 100);
        assert_eq!(metrics.snapshots_received, 1);
        assert_eq!(metrics.gaps_detected, 1);
        assert_eq!(metrics.successful_recoveries, 1);
        assert!(metrics.avg_update_latency > 0);
    }

    #[test]
    fn test_health_check() {
        let monitor = MonitoringSystem::new("ETHUSDT".to_string(), "Binance".to_string());

        // Record recent update
        monitor.record_update(100, 1024);

        // Perform health check
        let health = monitor.health_check(5000);
        assert!(matches!(health.status, HealthStatus::Healthy));
        assert!(health.orderbook_synchronized);
        assert!(health.last_update_age_ms < 1000);
    }

    #[test]
    fn test_latency_tracker() {
        let mut tracker = LatencyTracker::new(100);

        // Record samples
        for i in 1..=1000 {
            tracker.record(i * 10);
        }

        // Check percentiles
        let p50 = tracker.percentile(50.0);
        let p95 = tracker.percentile(95.0);
        let p99 = tracker.percentile(99.0);

        assert!(p50 < p95);
        assert!(p95 < p99);
        assert!(tracker.average() > 0);
    }

    #[test]
    fn test_prometheus_export() {
        let monitor = MonitoringSystem::new("BTCUSDT".to_string(), "Binance".to_string());

        monitor.record_update(100, 1024);
        monitor.record_gap(3);

        let prometheus = monitor.export_prometheus();
        assert!(prometheus.contains("orderbook_updates_total"));
        assert!(prometheus.contains("orderbook_gaps_total"));
        assert!(prometheus.contains("orderbook_latency_microseconds"));
    }
}