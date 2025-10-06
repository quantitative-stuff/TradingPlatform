use tokio::net::UdpSocket;
use std::net::Ipv4Addr;
use colored::*;
use serde_json;
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH, Instant};

#[derive(Debug, Clone)]
struct LatencyMetrics {
    network_delay: i64,      // Time from exchange timestamp to local receive
    processing_delay: i64,   // Time within our system
    total_delay: i64,
    timestamp: i64,
    had_inversion: bool,
}

#[derive(Debug, Default)]
struct CorrelationAnalysis {
    samples: VecDeque<LatencyMetrics>,
    inversions_by_delay: HashMap<String, u64>, // Bucket delays to see correlation
    high_latency_inversions: u64,
    low_latency_inversions: u64,
    burst_inversions: u64,
    isolated_inversions: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🔬 LATENCY CORRELATION ANALYZER".bright_cyan().bold());
    println!("Analyzing if packet inversions correlate with network latency...\n");
    
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    let multicast_addr: Ipv4Addr = "239.1.1.1".parse()?;
    let interface = Ipv4Addr::new(0, 0, 0, 0);
    socket.join_multicast_v4(multicast_addr, interface)?;
    
    println!("Connected to 239.1.1.1:9001");
    println!("Correlating inversions with network conditions...\n");
    
    let mut buf = vec![0u8; 65536];
    let mut last_timestamps: HashMap<String, i64> = HashMap::new();
    let mut correlations: HashMap<String, CorrelationAnalysis> = HashMap::new();
    
    let mut total_packets = 0u64;
    let mut recent_inversions: VecDeque<(String, i64)> = VecDeque::new();
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
                            total_packets += 1;
                            
                            let exchange = json["exchange"].as_str().unwrap_or("unknown");
                            let symbol = json["symbol"].as_str().unwrap_or("unknown");
                            let timestamp = json["timestamp"].as_i64().unwrap_or(0);
                            
                            let key = format!("{}:{}", exchange, symbol);
                            
                            // Calculate network delay
                            let network_delay = receive_time - timestamp;
                            
                            // Get or create correlation analysis for this symbol
                            let analysis = correlations.entry(key.clone()).or_insert(CorrelationAnalysis::default());
                            
                            // Check for inversion
                            let mut had_inversion = false;
                            if let Some(&last_ts) = last_timestamps.get(&key) {
                                if timestamp < last_ts {
                                    had_inversion = true;
                                    let inversion_size = last_ts - timestamp;
                                    
                                    // Track inversion timing
                                    recent_inversions.push_back((key.clone(), receive_time));
                                    if recent_inversions.len() > 100 {
                                        recent_inversions.pop_front();
                                    }
                                    
                                    // Categorize by network delay
                                    if network_delay > 1000 {
                                        analysis.high_latency_inversions += 1;
                                    } else {
                                        analysis.low_latency_inversions += 1;
                                    }
                                    
                                    // Check if part of burst
                                    let recent_count = recent_inversions.iter()
                                        .filter(|(k, t)| k == &key && (receive_time - t) < 1000)
                                        .count();
                                    
                                    if recent_count > 3 {
                                        analysis.burst_inversions += 1;
                                    } else {
                                        analysis.isolated_inversions += 1;
                                    }
                                    
                                    // Bucket delays
                                    let delay_bucket = if network_delay < 10 {
                                        "0-10ms"
                                    } else if network_delay < 50 {
                                        "10-50ms"
                                    } else if network_delay < 100 {
                                        "50-100ms"
                                    } else if network_delay < 500 {
                                        "100-500ms"
                                    } else if network_delay < 1000 {
                                        "500-1000ms"
                                    } else {
                                        ">1000ms"
                                    };
                                    
                                    *analysis.inversions_by_delay.entry(delay_bucket.to_string()).or_insert(0) += 1;
                                }
                            }
                            
                            // Store metrics
                            let metrics = LatencyMetrics {
                                network_delay,
                                processing_delay: 0, // Could add internal processing time
                                total_delay: network_delay,
                                timestamp,
                                had_inversion,
                            };
                            
                            analysis.samples.push_back(metrics);
                            if analysis.samples.len() > 1000 {
                                analysis.samples.pop_front();
                            }
                            
                            last_timestamps.insert(key, timestamp);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Error: {}", e);
            }
            Err(_) => {
                // Timeout - continue
            }
        }
        
        // Generate correlation report every 20 seconds
        if last_report.elapsed().as_secs() >= 20 {
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", "🔬 LATENCY CORRELATION ANALYSIS".bright_cyan().bold());
            println!("{}", "═".repeat(80).bright_cyan());
            
            println!("\n⏱️  Runtime: {:.1}s", start_time.elapsed().as_secs_f64());
            println!("📦 Total packets analyzed: {}", total_packets);
            
            // Calculate overall correlations
            let mut total_inversions = 0u64;
            let mut high_latency_inversions = 0u64;
            let mut low_latency_inversions = 0u64;
            let mut burst_inversions = 0u64;
            let mut isolated_inversions = 0u64;
            let mut delay_distribution: HashMap<String, u64> = HashMap::new();
            
            for analysis in correlations.values() {
                high_latency_inversions += analysis.high_latency_inversions;
                low_latency_inversions += analysis.low_latency_inversions;
                burst_inversions += analysis.burst_inversions;
                isolated_inversions += analysis.isolated_inversions;
                
                for (bucket, count) in &analysis.inversions_by_delay {
                    *delay_distribution.entry(bucket.clone()).or_insert(0) += count;
                }
            }
            
            total_inversions = high_latency_inversions + low_latency_inversions;
            
            if total_inversions > 0 {
                println!("\n{} CORRELATION FINDINGS", "📊".bright_white());
                println!("{}", "─".repeat(40).bright_blue());
                
                println!("\n🌐 Network Latency Correlation:");
                println!("   High latency (>1s) inversions: {} ({:.1}%)",
                    high_latency_inversions,
                    (high_latency_inversions as f64 / total_inversions as f64) * 100.0
                );
                println!("   Low latency (<1s) inversions:  {} ({:.1}%)",
                    low_latency_inversions,
                    (low_latency_inversions as f64 / total_inversions as f64) * 100.0
                );
                
                if high_latency_inversions > low_latency_inversions {
                    println!("   {} Strong correlation with network latency!", "⚠️".yellow());
                } else {
                    println!("   {} Inversions occur even with low latency", "📍".cyan());
                }
                
                println!("\n📈 Inversion Patterns:");
                println!("   Burst inversions (clustered):   {} ({:.1}%)",
                    burst_inversions,
                    (burst_inversions as f64 / total_inversions as f64) * 100.0
                );
                println!("   Isolated inversions (sporadic): {} ({:.1}%)",
                    isolated_inversions,
                    (isolated_inversions as f64 / total_inversions as f64) * 100.0
                );
                
                if burst_inversions > isolated_inversions {
                    println!("   {} Inversions tend to occur in bursts", "🔴".red());
                } else {
                    println!("   {} Inversions are randomly distributed", "🟡".yellow());
                }
                
                println!("\n📊 Delay Distribution of Inversions:");
                let buckets = ["0-10ms", "10-50ms", "50-100ms", "100-500ms", "500-1000ms", ">1000ms"];
                for bucket in &buckets {
                    if let Some(count) = delay_distribution.get(*bucket) {
                        let pct = (*count as f64 / total_inversions as f64) * 100.0;
                        let bar_len = (pct as usize).min(40);
                        let bar = "█".repeat(bar_len);
                        println!("   {:>10} {} {:.1}% ({})",
                            bucket,
                            bar.bright_blue(),
                            pct,
                            count
                        );
                    }
                }
                
                // Analyze specific symbols
                println!("\n🎯 Symbol-Specific Analysis:");
                let mut symbol_stats: Vec<_> = correlations.iter()
                    .filter(|(_, a)| a.high_latency_inversions + a.low_latency_inversions > 0)
                    .collect();
                symbol_stats.sort_by(|a, b| {
                    let a_total = a.1.high_latency_inversions + a.1.low_latency_inversions;
                    let b_total = b.1.high_latency_inversions + b.1.low_latency_inversions;
                    b_total.cmp(&a_total)
                });
                
                for (symbol, analysis) in symbol_stats.iter().take(5) {
                    let total = analysis.high_latency_inversions + analysis.low_latency_inversions;
                    let high_pct = if total > 0 {
                        (analysis.high_latency_inversions as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };
                    
                    println!("   {} - {} inversions ({:.0}% during high latency)",
                        symbol.yellow(),
                        total,
                        high_pct
                    );
                    
                    // Calculate average latency from recent samples
                    let recent_latencies: Vec<i64> = analysis.samples.iter()
                        .map(|m| m.network_delay)
                        .collect();
                    
                    if !recent_latencies.is_empty() {
                        let avg_latency = recent_latencies.iter().sum::<i64>() / recent_latencies.len() as i64;
                        let max_latency = *recent_latencies.iter().max().unwrap();
                        println!("      Avg latency: {}ms, Max: {}ms", avg_latency, max_latency);
                    }
                }
                
                // Conclusions
                println!("\n💡 CONCLUSIONS:");
                
                if high_latency_inversions > total_inversions * 7 / 10 {
                    println!("   ⚠️  Most inversions occur during high network latency");
                    println!("   → Consider implementing adaptive buffering based on latency");
                } else if low_latency_inversions > total_inversions * 7 / 10 {
                    println!("   📍 Most inversions occur even with low latency");
                    println!("   → Issue likely at the exchange or feed source");
                } else {
                    println!("   🔄 Inversions occur across all latency ranges");
                    println!("   → Multiple factors contributing to ordering issues");
                }
                
                if burst_inversions > total_inversions * 6 / 10 {
                    println!("   🔴 Burst pattern suggests temporary network congestion");
                } else {
                    println!("   🟡 Random pattern suggests consistent ordering issues");
                }
                
            } else {
                println!("\n{} No inversions detected yet!", "✅".bright_green());
                
                // Show latency stats anyway
                let mut all_latencies: Vec<i64> = Vec::new();
                for analysis in correlations.values() {
                    for metric in &analysis.samples {
                        all_latencies.push(metric.network_delay);
                    }
                }
                
                if !all_latencies.is_empty() {
                    all_latencies.sort();
                    let avg = all_latencies.iter().sum::<i64>() / all_latencies.len() as i64;
                    let median = all_latencies[all_latencies.len() / 2];
                    let p95 = all_latencies[all_latencies.len() * 95 / 100];
                    let p99 = all_latencies[all_latencies.len() * 99 / 100];
                    
                    println!("\n📊 Network Latency Stats:");
                    println!("   Average: {}ms", avg);
                    println!("   Median:  {}ms", median);
                    println!("   P95:     {}ms", p95);
                    println!("   P99:     {}ms", p99);
                }
            }
            
            println!("\n{}", "═".repeat(80).bright_cyan());
            last_report = Instant::now();
        }
    }
}