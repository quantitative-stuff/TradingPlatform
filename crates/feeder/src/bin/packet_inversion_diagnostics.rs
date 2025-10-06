use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Comprehensive diagnostic tool based on docs/Data_Inversion.md
/// Checks for all possible causes of packet inversions

#[derive(Debug)]
struct DiagnosticResults {
    // Endianness checks
    endianness_issues: Vec<String>,
    
    // Packet size analysis
    oversized_packets: u64,
    fragmented_packets: u64,
    
    // Timing analysis
    actual_inversions: u64,  // Timestamps going backwards
    perceived_inversions: u64,  // Out-of-order but timestamps correct
    
    // Network analysis
    packet_drops: u64,
    buffer_overflows: u64,
    
    // Data integrity
    corrupted_packets: u64,
    parsing_errors: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔬 PACKET INVERSION DIAGNOSTIC TOOL".bright_cyan().bold());
    println!("Based on docs/Data_Inversion.md analysis\n");
    
    // Create socket with larger buffers
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    
    // Try to set larger socket buffers (OS-level)
    let sock_ref = socket2::SockRef::from(&socket);
    // Set receive buffer to 8MB
    match sock_ref.set_recv_buffer_size(8 * 1024 * 1024) {
        Ok(_) => println!("✅ Set receive buffer to 8MB"),
        Err(e) => println!("⚠️  Could not set receive buffer: {}", e),
    }
    
    // Check actual buffer size
    match sock_ref.recv_buffer_size() {
        Ok(size) => println!("📊 Actual receive buffer size: {} bytes", size),
        Err(e) => println!("❌ Could not get buffer size: {}", e),
    }
    
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("\nStarting diagnostic analysis...");
    println!("{}", "─".repeat(80).bright_blue());
    
    let mut buf = vec![0u8; 65536];
    let mut results = DiagnosticResults {
        endianness_issues: Vec::new(),
        oversized_packets: 0,
        fragmented_packets: 0,
        actual_inversions: 0,
        perceived_inversions: 0,
        packet_drops: 0,
        buffer_overflows: 0,
        corrupted_packets: 0,
        parsing_errors: 0,
    };
    
    let mut last_timestamps: HashMap<String, i64> = HashMap::new();
    let mut last_sequence: HashMap<String, u64> = HashMap::new();
    let mut packet_sequences: HashMap<String, Vec<(u64, i64)>> = HashMap::new();
    
    let mut total_packets = 0u64;
    let mut last_packet_count = 0u64;
    let mut last_check = Instant::now();
    let start_time = Instant::now();
    
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_millis(100),
            socket.recv_from(&mut buf)
        ).await {
            Ok(Ok((len, _addr))) => {
                total_packets += 1;
                let receive_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;
                
                // Check 1: Packet size (MTU issues)
                if len > 1400 {  // Close to typical MTU of 1500
                    results.oversized_packets += 1;
                    if len > 1500 {
                        results.fragmented_packets += 1;
                        println!("⚠️  Large packet detected: {} bytes (may fragment)", len);
                    }
                }
                
                // Check 2: Parse packet
                if let Ok(data) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    if parts.len() >= 2 && (parts[0] == "TRADE" || parts[0] == "ORDERBOOK") {
                        match serde_json::from_str::<serde_json::Value>(parts[1]) {
                            Ok(json) => {
                                let exchange = json["exchange"].as_str().unwrap_or("unknown");
                                let symbol = json["symbol"].as_str().unwrap_or("unknown");
                                let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                                
                                let key = format!("{}:{}", exchange, symbol);
                                
                                // Check 3: Endianness check (for timestamp)
                                // If timestamp seems way off, might be endianness issue
                                let now_millis = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as i64;
                                
                                if (now_millis - timestamp).abs() > 86400000 * 365 {
                                    // More than a year difference
                                    results.endianness_issues.push(format!(
                                        "Timestamp {} seems wrong (now: {})", 
                                        timestamp, now_millis
                                    ));
                                    
                                    // Try byte-swapped interpretation
                                    let bytes = timestamp.to_be_bytes();
                                    let swapped = i64::from_le_bytes(bytes);
                                    println!("🔄 Possible endianness issue: {} vs swapped {}", 
                                        timestamp, swapped);
                                }
                                
                                // Check 4: Sequence tracking
                                let seq = last_sequence.entry(key.clone()).or_insert(0);
                                *seq += 1;
                                
                                // Store sequence with timestamp for analysis
                                packet_sequences.entry(key.clone())
                                    .or_insert(Vec::new())
                                    .push((*seq, timestamp));
                                
                                // Keep only last 100 for memory
                                if packet_sequences[&key].len() > 100 {
                                    packet_sequences.get_mut(&key).unwrap().remove(0);
                                }
                                
                                // Check 5: Timestamp ordering
                                if let Some(&last_ts) = last_timestamps.get(&key) {
                                    if timestamp < last_ts {
                                        results.actual_inversions += 1;
                                        
                                        // Analyze if this is actual inversion or network reordering
                                        let sequences = &packet_sequences[&key];
                                        if sequences.len() >= 2 {
                                            let last_two: Vec<_> = sequences.iter().rev().take(2).collect();
                                            if last_two[0].0 < last_two[1].0 {
                                                // Sequence numbers are out of order
                                                results.perceived_inversions += 1;
                                                println!("🔀 Network reordering: {} seq {} arrived after seq {}",
                                                    key, last_two[0].0, last_two[1].0);
                                            }
                                        }
                                        
                                        let diff = last_ts - timestamp;
                                        println!("⚠️  Timestamp inversion: {} went back {}ms", 
                                            key.yellow(), diff.to_string().red());
                                    }
                                }
                                
                                last_timestamps.insert(key, timestamp);
                            }
                            Err(e) => {
                                results.parsing_errors += 1;
                                println!("❌ JSON parse error: {}", e);
                            }
                        }
                    }
                } else {
                    results.corrupted_packets += 1;
                    println!("❌ Corrupted packet (not UTF-8)");
                    
                    // Show hex dump of first 32 bytes
                    print!("   Hex: ");
                    for i in 0..32.min(len) {
                        print!("{:02X} ", buf[i]);
                    }
                    println!();
                }
            }
            Ok(Err(e)) => {
                eprintln!("Socket error: {}", e);
            }
            Err(_) => {
                // Timeout - check for packet drops
                if total_packets == last_packet_count && total_packets > 0 {
                    // No packets in 100ms when we were receiving them
                    results.packet_drops += 1;
                }
            }
        }
        
        // Check for buffer overflow symptoms every second
        if last_check.elapsed().as_secs() >= 1 {
            let packets_per_sec = total_packets - last_packet_count;
            
            // If we suddenly drop from high rate to zero, might be buffer overflow
            if last_packet_count > 0 && packets_per_sec == 0 && last_packet_count > 100 {
                results.buffer_overflows += 1;
                println!("⚠️  Possible buffer overflow (sudden packet stop)");
            }
            
            last_packet_count = total_packets;
            last_check = Instant::now();
        }
        
        // Report every 30 seconds
        if start_time.elapsed().as_secs() >= 30 && start_time.elapsed().as_secs() % 30 == 0 {
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", "📊 DIAGNOSTIC REPORT".bright_cyan().bold());
            println!("{}", "═".repeat(80).bright_cyan());
            
            println!("\n⏱️  Runtime: {:.1}s", start_time.elapsed().as_secs_f64());
            println!("📦 Total packets: {}", total_packets);
            
            println!("\n{} ENDIANNESS ANALYSIS", "🔄".bright_white());
            if results.endianness_issues.is_empty() {
                println!("   ✅ No endianness issues detected");
            } else {
                println!("   ⚠️  {} potential endianness issues", results.endianness_issues.len());
                for issue in results.endianness_issues.iter().take(3) {
                    println!("      {}", issue.yellow());
                }
            }
            
            println!("\n{} PACKET SIZE ANALYSIS", "📏".bright_white());
            println!("   Oversized (>1400 bytes): {}", results.oversized_packets);
            println!("   Potentially fragmented (>1500): {}", results.fragmented_packets);
            if results.fragmented_packets > 0 {
                println!("   {} Risk of fragmentation issues!", "⚠️".yellow());
            }
            
            println!("\n{} ORDERING ANALYSIS", "🔢".bright_white());
            println!("   Actual inversions (timestamps backwards): {}", 
                results.actual_inversions.to_string().red());
            println!("   Network reordering (sequence mismatch): {}", 
                results.perceived_inversions);
            
            let true_inversions = results.actual_inversions - results.perceived_inversions;
            if true_inversions > 0 {
                println!("   {} TRUE inversions at source: {}", 
                    "🔴".red(), true_inversions);
            }
            
            println!("\n{} NETWORK HEALTH", "🌐".bright_white());
            println!("   Suspected packet drops: {}", results.packet_drops);
            println!("   Possible buffer overflows: {}", results.buffer_overflows);
            println!("   Corrupted packets: {}", results.corrupted_packets);
            println!("   Parsing errors: {}", results.parsing_errors);
            
            println!("\n{} DIAGNOSIS", "💡".bright_white());
            
            if results.actual_inversions > results.perceived_inversions {
                println!("   🔴 Source is sending packets with backwards timestamps!");
                println!("      → Check exchange adapter timestamp handling");
            }
            
            if results.perceived_inversions > results.actual_inversions / 2 {
                println!("   🟡 Significant network reordering occurring");
                println!("      → Consider implementing sequence numbers");
            }
            
            if results.oversized_packets > total_packets / 100 {
                println!("   ⚠️  Many oversized packets risk fragmentation");
                println!("      → Reduce packet size to <1400 bytes");
            }
            
            if results.buffer_overflows > 0 {
                println!("   ⚠️  Buffer overflows detected");
                println!("      → Increase socket buffer size (SO_RCVBUF)");
            }
            
            if results.corrupted_packets > 0 {
                println!("   ❌ Data corruption detected");
                println!("      → Check network hardware/drivers");
            }
            
            println!("\n{}", "═".repeat(80).bright_cyan());
        }
    }
}