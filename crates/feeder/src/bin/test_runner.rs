use anyhow::Result;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;
use std::io::{self, Write};
use tracing::{info, warn};

#[derive(Debug)]
enum TestCommand {
    FullSystem,
    Validator,
    ValidatorCompact,
    ValidatorChecklist,
    PerformanceMonitor,
    ConnectionMonitor,
    FullFeed,
    Monitoring,
    UdpDebug,
    ConnectionStats,
    Simple,
}

impl TestCommand {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "1" | "full-system" => Some(Self::FullSystem),
            "2" | "validator" => Some(Self::Validator),
            "3" | "validator-compact" => Some(Self::ValidatorCompact),
            "4" | "validator-checklist" => Some(Self::ValidatorChecklist),
            "5" | "performance" => Some(Self::PerformanceMonitor),
            "6" | "connection" => Some(Self::ConnectionMonitor),
            "7" | "full-feed" => Some(Self::FullFeed),
            "8" | "monitoring" => Some(Self::Monitoring),
            "9" | "udp-debug" => Some(Self::UdpDebug),
            "10" | "stats" => Some(Self::ConnectionStats),
            "11" | "simple" => Some(Self::Simple),
            _ => None,
        }
    }
}

async fn print_menu() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    FEEDER TEST RUNNER                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("Select a test to run:");
    println!();
    println!("  1) full-system         - Run feeder + validator (detailed)");
    println!("  2) validator           - Run feeder + validator (detailed mode)");
    println!("  3) validator-compact   - Run feeder + validator (compact mode)");
    println!("  4) validator-checklist - Run feeder + validator (checklist mode)");
    println!("  5) performance         - Run performance monitor with feeder");
    println!("  6) connection          - Test connection monitoring");
    println!("  7) full-feed           - Test full feed with all exchanges");
    println!("  8) monitoring          - Test monitoring system");
    println!("  9) udp-debug           - Debug UDP packet sending");
    println!("  10) stats              - Show connection statistics");
    println!("  11) simple             - Simple feeder test");
    println!();
    println!("  q) quit             - Exit test runner");
    println!();
    print!("Enter choice: ");
    io::stdout().flush().unwrap();
}

async fn run_full_system_test() -> Result<()> {
    println!("\n=== FULL SYSTEM TEST ===");
    println!("This test will run feeder with validator\n");
    
    // Set LIMITED_MODE for faster testing
    std::env::set_var("LIMITED_MODE", "true");
    println!("Set LIMITED_MODE=true for testing (10 symbols per category)");
    
    // Start feeder in background
    println!("Starting feeder (logs to file)...");
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "feeder"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    
    println!("Feeder started with PID: {}", feeder.id());
    println!("Waiting 10 seconds for feeder to initialize connections...");
    sleep(Duration::from_secs(10)).await;
    
    // Check log status
    if let Ok(status) = std::fs::read_to_string("logs/feeder_status.txt") {
        println!("\nFeeder status:");
        for line in status.lines() {
            println!("  {}", line);
        }
    }
    
    // Start validator
    println!("\nStarting validator (detailed mode)...");
    println!("Press Ctrl+C in validator to stop\n");
    
    let mut validator = Command::new("cargo")
        .args(&["run", "--bin", "validator", "detailed"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = validator.wait();
    
    println!("\nStopping feeder...");
    let _ = feeder.kill();
    
    println!("Test completed!");
    Ok(())
}

async fn run_performance_monitor_test() -> Result<()> {
    println!("\n=== PERFORMANCE MONITOR TEST ===");
    println!("This test will run the performance monitor alongside the feeder\n");
    
    // Start performance monitor
    println!("Starting performance monitor...");
    let mut monitor = Command::new("cargo")
        .args(&["run", "--bin", "performance_monitor"])
        .spawn()?;
    
    sleep(Duration::from_secs(2)).await;
    
    // Start feeder
    println!("Starting feeder...");
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "feeder"])
        .spawn()?;
    
    println!("\nSystem is running with performance monitoring enabled.");
    println!("The monitor will display:");
    println!("  - Real-time latency metrics (P50, P95, P99)");
    println!("  - Throughput and bandwidth usage");
    println!("  - Packet loss statistics");
    println!("  - Connection health metrics");
    println!("  - CPU and memory usage");
    println!("  - Active alerts and warnings\n");
    println!("Press Enter to stop...");
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    
    println!("Stopping processes...");
    let _ = feeder.kill();
    let _ = monitor.kill();
    
    println!("Test completed!");
    Ok(())
}

async fn run_connection_monitor_test() -> Result<()> {
    println!("\n=== CONNECTION MONITOR TEST ===");
    println!("Starting feeder with connection monitoring...");
    println!("The connection status will be shown every 1 minute");
    println!("Data updates will be shown every 5 seconds");
    println!("Press Ctrl+C to stop\n");
    
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "feeder"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = feeder.wait();
    Ok(())
}

async fn run_validator_test(mode: &str) -> Result<()> {
    println!("\n=== VALIDATOR TEST ({} mode) ===", mode.to_uppercase());
    println!("Running feeder with WebSocket validator\n");
    
    // Set LIMITED_MODE for faster testing
    std::env::set_var("LIMITED_MODE", "true");
    println!("Set LIMITED_MODE=true for testing");
    
    // Start feeder in background
    println!("Starting feeder (logs to file)...");
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "feeder"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    
    println!("Feeder started with PID: {}", feeder.id());
    println!("Waiting 10 seconds for initialization...");
    sleep(Duration::from_secs(10)).await;
    
    // Start validator with specified mode
    println!("\nStarting validator in {} mode...", mode);
    println!("Press Ctrl+C to stop\n");
    
    let mut validator = Command::new("cargo")
        .args(&["run", "--bin", "validator", mode])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = validator.wait();
    
    println!("\nStopping feeder...");
    let _ = feeder.kill();
    
    println!("Test completed!");
    Ok(())
}


async fn run_monitoring_test() -> Result<()> {
    println!("\n=== MONITORING TEST ===");
    println!("Starting monitor console...\n");
    
    let mut monitor = Command::new("cargo")
        .args(&["run", "--bin", "monitor_console"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = monitor.wait();
    Ok(())
}

async fn run_udp_debug_test() -> Result<()> {
    println!("\n=== UDP DEBUG TEST ===");
    println!("Starting UDP listener for debugging...\n");
    
    let mut listener = Command::new("cargo")
        .args(&["run", "--bin", "test_udp_listener"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = listener.wait();
    Ok(())
}

async fn run_connection_stats_test() -> Result<()> {
    println!("\n=== CONNECTION STATS TEST ===");
    println!("Showing connection statistics...\n");
    
    let mut stats = Command::new("cargo")
        .args(&["run", "--bin", "show_stats"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = stats.wait();
    Ok(())
}

async fn run_simple_test() -> Result<()> {
    println!("\n=== SIMPLE FEEDER TEST ===");
    println!("Running basic feeder test...\n");
    
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "test_feeder"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    let _ = feeder.wait();
    Ok(())
}

async fn run_full_feed_test() -> Result<()> {
    println!("\n=== FULL FEED TEST ===");
    println!("Testing full feed with all exchanges...\n");
    
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "feeder"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    println!("Running for 60 seconds...");
    sleep(Duration::from_secs(60)).await;
    
    println!("Stopping feeder...");
    let _ = feeder.kill();
    println!("Test completed!");
    Ok(())
}


#[tokio::main]
async fn main() -> Result<()> {
    loop {
        print_menu().await;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        
        if input == "q" || input == "quit" {
            println!("Exiting test runner...");
            break;
        }
        
        match TestCommand::from_str(input) {
            Some(TestCommand::FullSystem) => run_full_system_test().await?,
            Some(TestCommand::Validator) => run_validator_test("detailed").await?,
            Some(TestCommand::ValidatorCompact) => run_validator_test("compact").await?,
            Some(TestCommand::ValidatorChecklist) => run_validator_test("checklist").await?,
            Some(TestCommand::PerformanceMonitor) => run_performance_monitor_test().await?,
            Some(TestCommand::ConnectionMonitor) => run_connection_monitor_test().await?,
            Some(TestCommand::FullFeed) => run_full_feed_test().await?,
            Some(TestCommand::Monitoring) => run_monitoring_test().await?,
            Some(TestCommand::UdpDebug) => run_udp_debug_test().await?,
            Some(TestCommand::ConnectionStats) => run_connection_stats_test().await?,
            Some(TestCommand::Simple) => run_simple_test().await?,
            None => {
                println!("Invalid choice. Please try again.");
                continue;
            }
        }
        
        println!("\nPress Enter to continue...");
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
    }
    
    Ok(())
}