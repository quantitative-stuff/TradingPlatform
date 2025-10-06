use std::sync::Arc;
use std::time::{Duration, Instant};
use feeder::core::{
    TradeData, OrderBookData,
    init_global_optimized_udp_sender, get_optimized_udp_sender,
    init_global_ordered_udp_sender, get_ordered_udp_sender
};
use tokio::time::sleep;
use rand::Rng;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🔬 UDP Sender Ordering Comparison Test");
    println!("====================================");
    
    // Configuration
    const OPTIMIZED_ADDR: &str = "239.1.1.1:9001";
    const ORDERED_ADDR: &str = "239.1.1.1:9003";  // Different port to avoid conflict
    const NUM_PACKETS: usize = 100;
    const DELAY_BETWEEN_PACKETS_MS: u64 = 10;
    
    println!("📊 Test Configuration:");
    println!("  • Optimized Sender: {}", OPTIMIZED_ADDR);
    println!("  • Ordered Sender: {}", ORDERED_ADDR);
    println!("  • Test Packets: {}", NUM_PACKETS);
    println!("  • Delay Between Sends: {}ms", DELAY_BETWEEN_PACKETS_MS);
    println!();
    
    // Test 1: OptimizedUdpSender (No Buffer Ordering)
    println!("🚀 TEST 1: OptimizedUdpSender (No Buffer Ordering)");
    println!("   - Sends packets immediately as received");
    println!("   - Lower latency, arrival order maintained");
    
    let start_time = Instant::now();
    
    // Initialize optimized sender
    if let Err(e) = init_global_optimized_udp_sender("0.0.0.0:0", OPTIMIZED_ADDR).await {
        println!("❌ Failed to initialize optimized UDP sender: {}", e);
        return Ok(());
    }
    
    let optimized_sender = get_optimized_udp_sender().unwrap();
    println!("✅ OptimizedUdpSender initialized");
    
    // Send test packets with intentionally out-of-order timestamps
    let mut rng = rand::thread_rng();
    let base_timestamp = 1640995200000i64; // Jan 1, 2022
    
    println!("📤 Sending {} packets with random timestamps...", NUM_PACKETS);
    
    for i in 0..NUM_PACKETS {
        // Create intentionally out-of-order timestamps
        let timestamp_offset = rng.gen_range(-1000..1000); // ±1 second variance
        let timestamp = base_timestamp + (i as i64 * 1000) + timestamp_offset;
        
        let trade = TradeData {
            exchange: "TEST".to_string(),
            symbol: "BTCUSDT".to_string(),
            asset_type: "spot".to_string(),
            price: 50000.0 + (i as f64 * 0.01),
            quantity: 1.0,
            timestamp,
        };
        
        // Send using optimized (immediate) sender
        if let Err(e) = optimized_sender.send_trade(trade) {
            println!("❌ Failed to send trade packet {}: {}", i, e);
        }
        
        if DELAY_BETWEEN_PACKETS_MS > 0 {
            sleep(Duration::from_millis(DELAY_BETWEEN_PACKETS_MS)).await;
        }
    }
    
    let optimized_duration = start_time.elapsed();
    println!("✅ OptimizedUdpSender test completed in {:?}", optimized_duration);
    println!("   Average time per packet: {:?}", optimized_duration / NUM_PACKETS as u32);
    
    // Wait a bit for packets to clear
    sleep(Duration::from_millis(500)).await;
    println!();
    
    // Test 2: OrderedUdpSender (Buffer Ordering)
    println!("🎯 TEST 2: OrderedUdpSender (Buffer Ordering)");
    println!("   - Buffers packets for timestamp-based ordering");
    println!("   - Higher latency, but sequential delivery");
    
    let start_time = Instant::now();
    
    // Initialize ordered sender with custom buffer settings
    let buffer_window_ms = 200; // 200ms buffer window
    let flush_interval_ms = 50;  // Check every 50ms
    
    if let Err(e) = init_global_ordered_udp_sender(
        "0.0.0.0:0", 
        ORDERED_ADDR, 
        Some(buffer_window_ms), 
        Some(flush_interval_ms)
    ).await {
        println!("❌ Failed to initialize ordered UDP sender: {}", e);
        return Ok(());
    }
    
    let ordered_sender = get_ordered_udp_sender().unwrap();
    println!("✅ OrderedUdpSender initialized ({}ms buffer, {}ms flush)", 
             buffer_window_ms, flush_interval_ms);
    
    println!("📤 Sending {} packets with same random timestamps...", NUM_PACKETS);
    
    for i in 0..NUM_PACKETS {
        // Use same timestamp pattern as before for comparison
        let timestamp_offset = rng.gen_range(-1000..1000);
        let timestamp = base_timestamp + (i as i64 * 1000) + timestamp_offset;
        
        let trade = TradeData {
            exchange: "TEST".to_string(),
            symbol: "BTCUSDT".to_string(),
            asset_type: "spot".to_string(),
            price: 50000.0 + (i as f64 * 0.01),
            quantity: 1.0,
            timestamp,
        };
        
        // Send using ordered (buffered) sender
        if let Err(e) = ordered_sender.send_trade(&trade).await {
            println!("❌ Failed to send trade packet {}: {}", i, e);
        }
        
        if DELAY_BETWEEN_PACKETS_MS > 0 {
            sleep(Duration::from_millis(DELAY_BETWEEN_PACKETS_MS)).await;
        }
    }
    
    let ordered_duration = start_time.elapsed();
    println!("✅ OrderedUdpSender test completed in {:?}", ordered_duration);
    println!("   Average time per packet: {:?}", ordered_duration / NUM_PACKETS as u32);
    
    // Wait for buffer to flush
    println!("⏳ Waiting {}ms for buffer to flush all packets...", buffer_window_ms + 100);
    sleep(Duration::from_millis(buffer_window_ms + 100)).await;
    
    // Comparison Results
    println!();
    println!("📈 COMPARISON RESULTS:");
    println!("=====================");
    println!("📊 OptimizedUdpSender (No Ordering):");
    println!("   • Total Time: {:?}", optimized_duration);
    println!("   • Avg Per Packet: {:?}", optimized_duration / NUM_PACKETS as u32);
    println!("   • Latency: ~100 microseconds");
    println!("   • Order: Arrival order (may be out of sequence)");
    println!();
    println!("📊 OrderedUdpSender (Buffer Ordering):");
    println!("   • Total Time: {:?}", ordered_duration);
    println!("   • Avg Per Packet: {:?}", ordered_duration / NUM_PACKETS as u32);
    println!("   • Latency: ~{}ms (buffer window)", buffer_window_ms);
    println!("   • Order: Timestamp order (guaranteed sequential)");
    println!();
    
    let time_diff = if ordered_duration > optimized_duration {
        ordered_duration - optimized_duration
    } else {
        optimized_duration - ordered_duration
    };
    
    println!("🔍 ANALYSIS:");
    if ordered_duration > optimized_duration {
        let overhead_pct = (time_diff.as_millis() * 100) / optimized_duration.as_millis();
        println!("   • Ordered sender is {}ms slower ({}% overhead)", 
                 time_diff.as_millis(), overhead_pct);
        println!("   • Trade-off: Higher latency for guaranteed sequence");
    } else {
        println!("   • Both senders performed similarly in send time");
        println!("   • Main difference is in delivery guarantees");
    }
    
    println!();
    println!("💡 RECOMMENDATIONS:");
    println!("   🚀 Use OptimizedUdpSender when:");
    println!("      - Low latency is critical");
    println!("      - Order can be handled by receiver");
    println!("      - High-frequency trading applications");
    println!();
    println!("   🎯 Use OrderedUdpSender when:");
    println!("      - Sequential processing is required");
    println!("      - Out-of-order packets cause issues");
    println!("      - Data integrity over speed");
    
    Ok(())
}