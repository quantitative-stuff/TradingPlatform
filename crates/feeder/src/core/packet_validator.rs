use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use serde::{Serialize, Deserialize};
use once_cell::sync::OnceCell;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationMetrics {
    pub total_packets: u64,
    pub timestamp_inversions: u64,
    pub max_inversion_ms: i64,
    pub last_timestamp: i64,
    pub sequence_gaps: u64,
    pub last_sequence: u64,
    pub inversion_rate: f64,
}

impl Default for ValidationMetrics {
    fn default() -> Self {
        Self {
            total_packets: 0,
            timestamp_inversions: 0,
            max_inversion_ms: 0,
            last_timestamp: 0,
            sequence_gaps: 0,
            last_sequence: 0,
            inversion_rate: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PacketValidator {
    pre_send_metrics: Arc<Mutex<HashMap<String, ValidationMetrics>>>,
    post_receive_metrics: Arc<Mutex<HashMap<String, ValidationMetrics>>>,
    last_report: Arc<Mutex<Instant>>,
}

impl PacketValidator {
    pub fn new() -> Self {
        Self {
            pre_send_metrics: Arc::new(Mutex::new(HashMap::new())),
            post_receive_metrics: Arc::new(Mutex::new(HashMap::new())),
            last_report: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Validate packet BEFORE sending to UDP
    pub fn validate_pre_send(&self, exchange: &str, symbol: &str, timestamp: i64, sequence: Option<u64>) {
        let key = format!("{}:{}", exchange, symbol);
        let mut metrics = self.pre_send_metrics.lock().unwrap();
        let metric = metrics.entry(key.clone()).or_default();
        
        // Update packet count
        metric.total_packets += 1;
        
        // Check timestamp inversion
        if metric.last_timestamp > 0 && timestamp < metric.last_timestamp {
            metric.timestamp_inversions += 1;
            let inversion_ms = metric.last_timestamp - timestamp;
            if inversion_ms > metric.max_inversion_ms {
                metric.max_inversion_ms = inversion_ms;
            }
            
            // Log significant inversions immediately
            if inversion_ms > 100 {
                eprintln!("⚠️ PRE-SEND: Large timestamp inversion detected!");
                eprintln!("   Symbol: {} | Inversion: {}ms", key, inversion_ms);
                eprintln!("   Previous: {} | Current: {}", metric.last_timestamp, timestamp);
            }
        }
        
        // Check sequence gaps if provided
        if let Some(seq) = sequence {
            if metric.last_sequence > 0 && seq != metric.last_sequence + 1 {
                if seq > metric.last_sequence + 1 {
                    metric.sequence_gaps += 1;
                }
            }
            metric.last_sequence = seq;
        }
        
        metric.last_timestamp = timestamp;
        
        // Update inversion rate
        if metric.total_packets > 0 {
            metric.inversion_rate = (metric.timestamp_inversions as f64 / metric.total_packets as f64) * 100.0;
        }
    }

    /// Validate packet AFTER receiving from UDP (called by order_validator)
    pub fn validate_post_receive(&self, exchange: &str, symbol: &str, timestamp: i64, sequence: Option<u64>) {
        let key = format!("{}:{}", exchange, symbol);
        let mut metrics = self.post_receive_metrics.lock().unwrap();
        let metric = metrics.entry(key).or_default();
        
        // Same validation logic as pre-send
        metric.total_packets += 1;
        
        if metric.last_timestamp > 0 && timestamp < metric.last_timestamp {
            metric.timestamp_inversions += 1;
            let inversion_ms = metric.last_timestamp - timestamp;
            if inversion_ms > metric.max_inversion_ms {
                metric.max_inversion_ms = inversion_ms;
            }
        }
        
        if let Some(seq) = sequence {
            if metric.last_sequence > 0 && seq != metric.last_sequence + 1 {
                if seq > metric.last_sequence + 1 {
                    metric.sequence_gaps += 1;
                }
            }
            metric.last_sequence = seq;
        }
        
        metric.last_timestamp = timestamp;
        
        if metric.total_packets > 0 {
            metric.inversion_rate = (metric.timestamp_inversions as f64 / metric.total_packets as f64) * 100.0;
        }
    }

    /// Compare pre-send and post-receive metrics
    pub fn get_comparison_report(&self) -> String {
        let pre_metrics = self.pre_send_metrics.lock().unwrap();
        let post_metrics = self.post_receive_metrics.lock().unwrap();
        
        let mut report = String::new();
        report.push_str("\n📊 PACKET VALIDATION COMPARISON REPORT\n");
        report.push_str("=====================================\n\n");
        
        // Aggregate statistics
        let mut total_pre_inversions = 0u64;
        let mut total_post_inversions = 0u64;
        let mut total_pre_packets = 0u64;
        let mut total_post_packets = 0u64;
        let mut network_induced_inversions = 0i64;
        
        for (key, pre_metric) in pre_metrics.iter() {
            total_pre_inversions += pre_metric.timestamp_inversions;
            total_pre_packets += pre_metric.total_packets;
            
            if let Some(post_metric) = post_metrics.get(key) {
                total_post_inversions += post_metric.timestamp_inversions;
                total_post_packets += post_metric.total_packets;
                
                let inversion_diff = post_metric.timestamp_inversions as i64 - pre_metric.timestamp_inversions as i64;
                if inversion_diff > 0 {
                    network_induced_inversions += inversion_diff;
                    
                    report.push_str(&format!(
                        "⚠️ {} - Network-induced inversions: +{}\n",
                        key, inversion_diff
                    ));
                    report.push_str(&format!(
                        "   Pre-send: {} inversions | Post-receive: {} inversions\n",
                        pre_metric.timestamp_inversions, post_metric.timestamp_inversions
                    ));
                }
            }
        }
        
        report.push_str("\n📈 SUMMARY:\n");
        report.push_str(&format!("Pre-send inversions:  {} / {} packets ({:.3}%)\n",
            total_pre_inversions, total_pre_packets,
            if total_pre_packets > 0 { (total_pre_inversions as f64 / total_pre_packets as f64) * 100.0 } else { 0.0 }
        ));
        report.push_str(&format!("Post-receive inversions: {} / {} packets ({:.3}%)\n",
            total_post_inversions, total_post_packets,
            if total_post_packets > 0 { (total_post_inversions as f64 / total_post_packets as f64) * 100.0 } else { 0.0 }
        ));
        
        if network_induced_inversions > 0 {
            report.push_str(&format!("\n🔴 Network/Multicast induced {} additional inversions!\n", network_induced_inversions));
            report.push_str("   → The network stack or multicast is reordering packets\n");
        } else if total_pre_inversions > 0 {
            report.push_str("\n🟡 Inversions exist at source (before sending)\n");
            report.push_str("   → Issue originates from exchange data or feeder processing\n");
        } else {
            report.push_str("\n🟢 No ordering issues detected at any stage\n");
        }
        
        report
    }

    /// Check if it's time to print comparison report
    pub fn maybe_print_report(&self, interval_secs: u64) {
        let mut last_report = self.last_report.lock().unwrap();
        if last_report.elapsed().as_secs() >= interval_secs {
            println!("{}", self.get_comparison_report());
            *last_report = Instant::now();
        }
    }
}

// Global validator instance
pub static GLOBAL_PACKET_VALIDATOR: OnceCell<Arc<PacketValidator>> = OnceCell::new();

pub fn init_packet_validator() {
    GLOBAL_PACKET_VALIDATOR.get_or_init(|| Arc::new(PacketValidator::new()));
}

pub fn get_packet_validator() -> Option<Arc<PacketValidator>> {
    GLOBAL_PACKET_VALIDATOR.get().cloned()
}

/// Convenience function for pre-send validation
pub fn validate_before_send(exchange: &str, symbol: &str, timestamp: i64) {
    if let Some(validator) = get_packet_validator() {
        validator.validate_pre_send(exchange, symbol, timestamp, None);
    }
}

/// Convenience function for post-receive validation
pub fn validate_after_receive(exchange: &str, symbol: &str, timestamp: i64) {
    if let Some(validator) = get_packet_validator() {
        validator.validate_post_receive(exchange, symbol, timestamp, None);
    }
}