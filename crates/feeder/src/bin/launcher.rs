// Simple launcher to run feeder and validator together
use anyhow::Result;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::thread;
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    
    // Parse command line arguments
    let mode = if args.len() > 1 {
        match args[1].as_str() {
            "udp" => "udp",
            "simple" => "simple",
            "compact" => "compact",
            "checklist" => "checklist",
            "detailed" => "detailed",
            _ => {
                println!("Usage: launcher [udp|simple|compact|detailed|checklist]");
                println!("Default: udp (receives data via UDP packets)");
                "udp"
            }
        }
    } else {
        "udp"
    };
    
    let limited = args.iter().any(|arg| arg == "--limited");
    
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              FEEDER & VALIDATOR LAUNCHER                      ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    
    if limited {
        env::set_var("LIMITED_MODE", "true");
        println!("📌 LIMITED MODE enabled (10 symbols per category)");
    }
    
    println!("📊 Validator mode: {}", mode);
    println!();
    
    // Start feeder in background
    println!("🚀 Starting feeder (logs to file)...");
    let mut feeder = Command::new("cargo")
        .args(&["run", "--bin", "feeder"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    
    let feeder_pid = feeder.id();
    println!("✅ Feeder started with PID: {}", feeder_pid);
    
    // Wait for initialization
    println!("⏳ Waiting 10 seconds for WebSocket connections...");
    for i in (1..=10).rev() {
        print!("\r   {} seconds remaining...  ", i);
        use std::io::{self, Write};
        io::stdout().flush()?;
        thread::sleep(Duration::from_secs(1));
    }
    println!("\r✅ Initialization complete!        ");
    println!();
    
    // Check if logs directory exists and show status
    if let Ok(status) = std::fs::read_to_string("logs/feeder_status.txt") {
        println!("📄 Feeder Status:");
        for line in status.lines() {
            println!("   {}", line);
        }
        println!();
    }
    
    // Start monitor based on mode
    let (bin_name, mode_arg) = match mode {
        "udp" => {
            println!("🔍 Starting UDP monitor (receives packets from feeder)...");
            ("udp_monitor", None)
        },
        "simple" => {
            println!("🔍 Starting simple monitor (no scrolling)...");
            ("simple_monitor", None)
        },
        _ => {
            println!("🔍 Starting validator in {} mode...", mode);
            ("validator", Some(mode))
        }
    };
    
    println!("   Press Ctrl+C to stop both processes");
    println!();
    println!("{}", "─".repeat(65));
    println!();
    
    let mut monitor_args = vec!["run", "--bin", bin_name];
    if let Some(m) = mode_arg {
        monitor_args.push(m);
    }
    
    let mut validator = Command::new("cargo")
        .args(&monitor_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    
    // Wait for validator to exit (Ctrl+C)
    let _ = validator.wait();
    
    // Stop feeder
    println!();
    println!("{}", "─".repeat(65));
    println!();
    println!("🛑 Stopping feeder...");
    let _ = feeder.kill();
    let _ = feeder.wait();
    
    println!("✅ All processes stopped successfully!");
    println!();
    
    Ok(())
}