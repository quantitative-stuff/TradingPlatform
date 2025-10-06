use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Analyzes packet ordering at TWO points:
/// 1. BEFORE sending to multicast (at source)
/// 2. AFTER receiving from multicast (at destination)
/// This helps identify WHERE inversions occur

#[derive(Debug, Clone)]
struct PacketTrace {
    symbol: String,
    timestamp: i64,
    send_time: i64,      // When we sent it
    receive_time: i64,    // When we received it
    sequence: u64,        // Packet sequence number
}

#[derive(Debug, Default)]
struct OrderingStats {
    // Pre-send stats (at source)
    source_inversions: u64,
    source_packets: u64,
    source_last_timestamps: HashMap<String, i64>,
    
    // Post-receive stats (after multicast)
    network_inversions: u64,
    network_packets: u64,
    receive_last_timestamps: HashMap<String, i64>,
    
    // Correlation data
    sent_packets: HashMap<String, Vec<PacketTrace>>,
    received_packets: HashMap<String, Vec<PacketTrace>>,
    
    // Analysis
    pure_network_inversions: u64,  // Inversions that didn't exist at source
    source_propagated: u64,         // Inversions that existed at source
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔄 DUAL-SIDE ORDER ANALYZER".bright_cyan().bold());
    println!("Tracking packet order BEFORE send and AFTER receive...\n");
    
    let stats = Arc::new(Mutex::new(OrderingStats::default()));
    let stats_clone = stats.clone();
    
    // Task 1: Monitor PRE-SEND (intercept before multicast)
    let pre_send_task = tokio::spawn(async move {
        monitor_pre_send(stats_clone).await;
    });
    
    // Task 2: Monitor POST-RECEIVE (from multicast)
    let post_receive_task = tokio::spawn(async move {
        monitor_post_receive(stats).await;
    });
    
    // Wait for both monitors
    tokio::select! {
        _ = pre_send_task => {},
        _ = post_receive_task => {},
    }
    
    Ok(())
}

async fn monitor_pre_send(stats: Arc<Mutex<OrderingStats>>) {
    println!("{} Starting PRE-SEND monitor (source analysis)...", "📤".bright_green());
    
    // This would ideally hook into your actual send pipeline
    // For now, we'll monitor a secondary port to simulate
    let socket = match UdpSocket::bind("0.0.0.0:9003").await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to bind pre-send monitor: {}", e);
            return;
        }
    };
    
    println!("Pre-send monitor listening on port 9003");
    println!("NOTE: To fully test, modify your feeder to also send to 127.0.0.1:9003\n");
    
    let mut buf = vec![0u8; 65536];
    let mut sequence = 0u64;
    
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, _addr)) => {
                let send_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;
                
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            sequence += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown");
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            let key = format!("{}:{}", exchange, symbol);
                            
                            let mut stats_guard = stats.lock().await;
                            stats_guard.source_packets += 1;
                            
                            // Check for source inversion
                            if let Some(&last_ts) = stats_guard.source_last_timestamps.get(&key) {
                                if timestamp < last_ts {
                                    stats_guard.source_inversions += 1;
                                    println!("{} SOURCE INVERSION: {} went back {}ms",
                                        "⚠️".red(),
                                        key.yellow(),
                                        (last_ts - timestamp).to_string().red()
                                    );
                                }
                            }
                            
                            stats_guard.source_last_timestamps.insert(key.clone(), timestamp);
                            
                            // Track packet for correlation
                            let trace = PacketTrace {
                                symbol: key.clone(),
                                timestamp,
                                send_time,
                                receive_time: 0,
                                sequence,
                            };
                            
                            stats_guard.sent_packets
                                .entry(key)
                                .or_insert(Vec::new())
                                .push(trace);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Pre-send monitor error: {}", e);
            }
        }
    }
}

async fn monitor_post_receive(stats: Arc<Mutex<OrderingStats>>) {
    println!("{} Starting POST-RECEIVE monitor (after multicast)...", "📥".bright_blue());
    
    // Connect to actual multicast
    let socket = match UdpSocket::bind("0.0.0.0:9001").await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to bind post-receive monitor: {}", e);
            return;
        }
    };
    
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse().unwrap();
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface).unwrap();
    
    println!("Post-receive monitor connected to multicast 239.1.1.1:9001\n");
    println!("{}", "─".repeat(80).bright_blue());
    
    let mut buf = vec![0u8; 65536];
    let mut sequence = 0u64;
    let start_time = Instant::now();
    let mut last_report = Instant::now();
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, _addr))) => {
                let receive_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;
                
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(parts[1]) {
                            sequence += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown");
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            let key = format!("{}:{}", exchange, symbol);
                            
                            let mut stats_guard = stats.lock().await;
                            stats_guard.network_packets += 1;
                            
                            // Check for network inversion
                            let mut is_network_inversion = false;
                            if let Some(&last_ts) = stats_guard.receive_last_timestamps.get(&key) {
                                if timestamp < last_ts {
                                    stats_guard.network_inversions += 1;
                                    is_network_inversion = true;
                                    
                                    // Check if this inversion existed at source
                                    let existed_at_source = check_if_source_inversion(
                                        &stats_guard.sent_packets.get(&key),
                                        timestamp,
                                        last_ts
                                    );
                                    
                                    if existed_at_source {
                                        stats_guard.source_propagated += 1;
                                        println!("{} PROPAGATED INVERSION: {} (existed at source)",
                                            "🔄".yellow(),
                                            key.yellow()
                                        );
                                    } else {
                                        stats_guard.pure_network_inversions += 1;
                                        println!("{} NETWORK INVERSION: {} (NEW, caused by network!)",
                                            "🌐".bright_red(),
                                            key.bright_red()
                                        );
                                    }
                                }
                            }
                            
                            stats_guard.receive_last_timestamps.insert(key.clone(), timestamp);
                            
                            // Track packet
                            let trace = PacketTrace {
                                symbol: key.clone(),
                                timestamp,
                                send_time: 0,
                                receive_time,
                                sequence,
                            };
                            
                            stats_guard.received_packets
                                .entry(key)
                                .or_insert(Vec::new())
                                .push(trace);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Receive error: {}", e);
            }
            Err(_) => {
                // Timeout - continue
            }
        }
        
        // Report every 20 seconds
        if last_report.elapsed().as_secs() >= 20 {
            print_analysis_report(&stats, start_time.elapsed().as_secs()).await;
            last_report = Instant::now();
        }
    }
}

fn check_if_source_inversion(
    sent_packets: &Option<&Vec<PacketTrace>>,
    current_ts: i64,
    previous_ts: i64
) -> bool {
    if let Some(packets) = sent_packets {
        // Check if this inversion pattern existed in sent packets
        for window in packets.windows(2) {
            if window[0].timestamp == previous_ts && window[1].timestamp == current_ts {
                return true;  // Same inversion existed at source
            }
        }
    }
    false
}

async fn print_analysis_report(stats: &Arc<Mutex<OrderingStats>>, elapsed: u64) {
    let stats_guard = stats.lock().await;
    
    print!("\x1B[2J\x1B[1;1H");
    println!("{}", "🔄 DUAL-SIDE ORDER ANALYSIS REPORT".bright_cyan().bold());
    println!("{}", "═".repeat(80).bright_cyan());
    
    println!("\n⏱️  Runtime: {} seconds", elapsed);
    
    // Source analysis
    println!("\n{} SOURCE (Pre-Send) Analysis:", "📤".bright_green());
    println!("   Packets analyzed: {}", stats_guard.source_packets);
    println!("   Inversions at source: {}", stats_guard.source_inversions.to_string().yellow());
    if stats_guard.source_packets > 0 {
        let rate = (stats_guard.source_inversions as f64 / stats_guard.source_packets as f64) * 100.0;
        println!("   Source inversion rate: {:.3}%", rate);
    }
    
    // Network analysis
    println!("\n{} NETWORK (Post-Receive) Analysis:", "📥".bright_blue());
    println!("   Packets received: {}", stats_guard.network_packets);
    println!("   Total inversions: {}", stats_guard.network_inversions.to_string().red());
    if stats_guard.network_packets > 0 {
        let rate = (stats_guard.network_inversions as f64 / stats_guard.network_packets as f64) * 100.0;
        println!("   Network inversion rate: {:.3}%", rate);
    }
    
    // Correlation
    println!("\n{} CORRELATION ANALYSIS:", "🔍".bright_white());
    println!("   Inversions from source: {} (propagated through)",
        stats_guard.source_propagated.to_string().yellow()
    );
    println!("   NEW network inversions: {} (caused by UDP/network)",
        stats_guard.pure_network_inversions.to_string().bright_red()
    );
    
    if stats_guard.network_inversions > 0 {
        let network_pct = (stats_guard.pure_network_inversions as f64 / 
                          stats_guard.network_inversions as f64) * 100.0;
        println!("   Network contribution: {:.1}%", network_pct);
    }
    
    // Diagnosis
    println!("\n{} DIAGNOSIS:", "💡".bright_white());
    
    if stats_guard.source_inversions == 0 && stats_guard.network_inversions == 0 {
        println!("   ✅ No inversions detected at any stage!");
    } else if stats_guard.pure_network_inversions == 0 && stats_guard.source_inversions > 0 {
        println!("   ⚠️  All inversions originate at SOURCE (exchange/feeder issue)");
        println!("   → Fix timestamp handling in exchange adapters");
    } else if stats_guard.pure_network_inversions > stats_guard.source_propagated {
        println!("   🌐 Most inversions caused by NETWORK reordering");
        println!("   → Consider buffering or sequence numbering");
    } else {
        println!("   🔄 Mixed causes - both source and network issues");
    }
    
    // Note about setup
    if stats_guard.source_packets == 0 {
        println!("\n{} NOTE: No pre-send data captured.", "⚠️".yellow());
        println!("   To monitor pre-send, modify feeder to also send to 127.0.0.1:9003");
        println!("   Or use the packet_validator in your feeder code.");
    }
    
    println!("\n{}", "═".repeat(80).bright_cyan());
}