// Test to verify multi-port sender initialization
use feeder::core::{init_global_multi_port_sender, get_multi_port_sender};

#[tokio::main]
async fn main() {
    println!("Testing multi-port sender initialization...");

    // Check before init
    println!("Before init: {:?}", get_multi_port_sender().is_some());

    // Initialize
    match init_global_multi_port_sender() {
        Ok(_) => println!("✅ Initialization successful"),
        Err(e) => println!("❌ Initialization failed: {}", e),
    }

    // Check after init
    println!("After init: {:?}", get_multi_port_sender().is_some());

    // Try to get it multiple times (should be lock-free)
    for i in 0..5 {
        let sender = get_multi_port_sender();
        println!("Attempt {}: {:?}", i, sender.is_some());
    }
}