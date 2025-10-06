// UDP Monitor - receives and displays binary and text UDP packets from feeder
use anyhow::Result;
use std::collections::HashMap;
use tokio::net::UdpSocket;
use std::time::{Duration, Instant};
use colored::*;

#[derive(Debug)]
struct ExchangeStats {
    trades: usize,
    orderbooks: usize,
    last_update: Option<Instant>,
    first_seen: Instant,
}

impl Default for ExchangeStats {
    fn default() -> Self {
        Self {
            trades: 0,
            orderbooks: 0,
            last_update: None,
            first_seen: Instant::now(),
        }
    }
}

#[derive(Debug)]
struct ConnectionStats {
    connected: usize,
    disconnected: usize,
    reconnect_count: usize,
    total_connections: usize,
    first_seen: Instant,
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            connected: 0,
            disconnected: 0,
            reconnect_count: 0,
            total_connections: 0,
            first_seen: Instant::now(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let monitor_start_time = Instant::now();
    println!("=== UDP Monitor (Binary Format) ===");
    println!("Listening on multicast 239.255.42.99:9001");
    println!("Press Ctrl+C to exit\n");

    // Bind to the monitor endpoint
    let socket = UdpSocket::bind("0.0.0.0:9001").await?;
    println!("✅ UDP socket bound to port 9001\n");

    // Join the multicast group to receive packets from feeder
    socket.join_multicast_v4("239.255.42.99".parse()?, "0.0.0.0".parse()?)?;
    println!("✅ Joined multicast group 239.255.42.99:9001\n");

    let mut exchange_stats: HashMap<String, ExchangeStats> = HashMap::new();
    let mut connection_stats: HashMap<String, ConnectionStats> = HashMap::new();
    let mut last_display = Instant::now();
    let mut total_packets = 0;
    let mut binary_packets = 0;
    let mut text_packets = 0;

    // Buffer for receiving UDP packets (64KB to handle large packets)
    let mut buf = vec![0u8; 65536];

    // Clear screen initially
    print!("\x1B[2J\x1B[H");

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, _addr)) => {
                        total_packets += 1;

                        // Check if it's a binary packet (starts with protocol version 1)
                        if len >= 58 && buf[0] == 1 {
                            binary_packets += 1;

                            // Binary packet - extract header info
                            // Header structure (58 bytes total):
                            // - protocol_version: u8 at offset 0
                            // - exchange_timestamp: u64 at offset 1
                            // - local_timestamp: u64 at offset 9
                            // - flags_and_count: u8 at offset 17
                            // - symbol: [u8; 20] at offset 18
                            // - exchange: [u8; 20] at offset 38

                            let flags_and_count = buf[17];
                            // Extract packet type from bits 4-6 (3 bits for type)
                            let packet_type = (flags_and_count >> 4) & 0x07;
                            // Extract item count from bits 0-3
                            let _item_count = flags_and_count & 0x0F;

                            // Extract symbol and exchange from header
                            let symbol_bytes = &buf[18..38];
                            let exchange_bytes = &buf[38..58];

                            let _symbol = String::from_utf8_lossy(symbol_bytes)
                                .trim_end_matches('\0')
                                .to_string();
                            let exchange = String::from_utf8_lossy(exchange_bytes)
                                .trim_end_matches('\0')
                                .to_string();

                            // Determine asset type from exchange name
                            let (exchange_name, asset_type) = if exchange.contains('_') {
                                let parts: Vec<&str> = exchange.split('_').collect();
                                (parts[0].to_string(), parts.get(1).unwrap_or(&"spot").to_string())
                            } else {
                                (exchange.clone(), "spot".to_string())
                            };

                            let key = format!("{}_{}", exchange_name, asset_type);
                            let stats = exchange_stats.entry(key).or_default();

                            // Update stats based on packet type
                            match packet_type {
                                0 => {
                                    // OrderBook packet
                                    stats.orderbooks += 1;
                                    stats.last_update = Some(Instant::now());
                                }
                                1 => {
                                    // Trade packet
                                    stats.trades += 1;
                                    stats.last_update = Some(Instant::now());
                                }
                                _ => {}
                            }
                        } else {
                            // Text packet - handle connection status
                            text_packets += 1;
                            let data = String::from_utf8_lossy(&buf[..len]);

                            // Handle batched packets (separated by newlines)
                            for packet_line in data.lines() {
                                if packet_line.is_empty() {
                                    continue;
                                }

                                // Parse packet
                                let parts: Vec<&str> = packet_line.split('|').collect();

                                match parts[0] {
                                    "CONN" if parts.len() >= 6 => {
                                        // Format: CONN|exchange|connected|disconnected|reconnect_count|total_connections
                                        let exchange = parts[1].to_string();
                                        let stats = connection_stats.entry(exchange).or_default();
                                        stats.connected = parts[2].parse().unwrap_or(0);
                                        stats.disconnected = parts[3].parse().unwrap_or(0);
                                        stats.reconnect_count = parts[4].parse().unwrap_or(0);
                                        stats.total_connections = parts[5].parse().unwrap_or(0);
                                    }
                                    _ => {
                                        // Unknown text packet type
                                    }
                                }
                            }
                        }

                        // Update display every 500ms
                        if last_display.elapsed() > Duration::from_millis(500) {
                            display_stats(&exchange_stats, &connection_stats, total_packets,
                                        binary_packets, text_packets, monitor_start_time);
                            last_display = Instant::now();
                        }
                    }
                    Err(e) => {
                        eprintln!("Error receiving UDP packet: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n\nMonitor stopped.");
                break;
            }
        }
    }

    Ok(())
}

fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

fn display_stats(
    exchange_stats: &HashMap<String, ExchangeStats>,
    connection_stats: &HashMap<String, ConnectionStats>,
    total_packets: usize,
    binary_packets: usize,
    text_packets: usize,
    monitor_start_time: Instant,
) {
    // Clear screen and move cursor to top
    print!("\x1B[2J\x1B[H");

    // Display header
    println!("{}", "═".repeat(80).bright_blue());
    let monitor_uptime = format_duration(monitor_start_time.elapsed());
    println!("{} {} | Uptime: {}",
        "UDP MONITOR (BINARY)".bright_cyan().bold(),
        chrono::Local::now().format("%H:%M:%S").to_string().bright_yellow(),
        monitor_uptime.bright_green()
    );
    println!("{}", "═".repeat(80).bright_blue());

    // Connection stats
    println!("\n{}", "📡 CONNECTIONS:".bright_green());

    if connection_stats.is_empty() {
        println!("  {}", "No connections reported yet...".yellow());
    } else {
        for (exchange, stats) in connection_stats.iter() {
            let status = if stats.connected > 0 {
                format!("{} ACTIVE", stats.connected).green()
            } else if stats.disconnected > 0 {
                format!("{} DISCONNECTED", stats.disconnected).red()
            } else {
                "CONNECTING".yellow()
            };

            let uptime = format_duration(stats.first_seen.elapsed());
            println!("  {:<12} : {} (reconn: {}, total: {}, up: {})",
                exchange.bright_white(),
                status,
                stats.reconnect_count,
                stats.total_connections,
                uptime.bright_cyan()
            );
        }
    }

    // Exchange data stats
    println!("\n{}", "📊 EXCHANGE DATA:".bright_green());

    if exchange_stats.is_empty() {
        println!("  {}", "No data received yet (waiting for binary packets)...".yellow());
    } else {
        let mut total_trades = 0;
        let mut total_orderbooks = 0;

        for (key, stats) in exchange_stats.iter() {
            let age = stats.last_update
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(999);

            let status_color = if age < 5 {
                "green"
            } else if age < 30 {
                "yellow"
            } else {
                "red"
            };

            let status = match status_color {
                "green" => "●".green(),
                "yellow" => "●".yellow(),
                _ => "●".red(),
            };

            let uptime = format_duration(stats.first_seen.elapsed());
            println!("  {} {:<20} T:{:<6} OB:{:<6} ({}s ago, up: {})",
                status,
                key.bright_white(),
                stats.trades.to_string().cyan(),
                stats.orderbooks.to_string().magenta(),
                age,
                uptime.bright_cyan()
            );

            total_trades += stats.trades;
            total_orderbooks += stats.orderbooks;
        }

        println!("\n  {} Trades: {} | OrderBooks: {}",
            "TOTAL:".bright_yellow().bold(),
            total_trades.to_string().bright_cyan(),
            total_orderbooks.to_string().bright_magenta()
        );
    }

    // Packet stats
    println!("\n{}", "📦 PACKET STATS:".bright_green());
    println!("  Total received: {}", total_packets.to_string().bright_cyan());
    println!("  Binary packets: {} ({:.1}%)",
        binary_packets.to_string().bright_magenta(),
        if total_packets > 0 { binary_packets as f64 / total_packets as f64 * 100.0 } else { 0.0 }
    );
    println!("  Text packets: {} ({:.1}%)",
        text_packets.to_string().bright_yellow(),
        if total_packets > 0 { text_packets as f64 / total_packets as f64 * 100.0 } else { 0.0 }
    );

    // Status
    let overall_status = if !exchange_stats.is_empty() {
        "✅ RECEIVING BINARY DATA".green().bold()
    } else if !connection_stats.is_empty() &&
        connection_stats.values().any(|s| s.connected > 0) {
        "⏳ CONNECTIONS ACTIVE (WAITING FOR DATA)".yellow()
    } else if total_packets > 0 {
        "⏳ RECEIVING PACKETS (NO DATA YET)".yellow()
    } else {
        "❌ NO PACKETS RECEIVED YET".red()
    };

    println!("\n{}", overall_status);

    // Padding
    for _ in 0..2 {
        println!("{}", " ".repeat(80));
    }
}