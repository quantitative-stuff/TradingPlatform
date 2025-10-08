use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::interval;
use tokio::task::JoinHandle;

use super::{TRADES, ORDERBOOKS, CONNECTION_STATS};

/// Terminal monitor that displays real-time WebSocket data
pub struct TerminalMonitor {
    enabled: bool,
    refresh_interval: Duration,
    last_update: Arc<RwLock<Instant>>,
    monitor_handle: Option<JoinHandle<()>>,
}

impl TerminalMonitor {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            refresh_interval: Duration::from_secs(2),
            last_update: Arc::new(RwLock::new(Instant::now())),
            monitor_handle: None,
        }
    }

    /// Start the monitor display loop
    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.enabled {
            // Silent mode - no console output
            return Ok(());
        }

        tracing::debug!("╔════════════════════════════════════════════════════════════════╗");
        tracing::debug!("║         WebSocket Feeder - Real-time Market Data Monitor      ║");
        tracing::debug!("╚════════════════════════════════════════════════════════════════╝");
        tracing::debug!("");
        tracing::debug!("Terminal display enabled. Data will appear once connections are established.");
        tracing::debug!("");

        let refresh_interval = self.refresh_interval;
        let last_update = self.last_update.clone();

        // Spawn monitor task
        let handle = tokio::spawn(async move {
            let mut ticker = interval(refresh_interval);
            let mut iteration = 0;
            let mut has_shown_data = false;

            loop {
                ticker.tick().await;
                iteration += 1;

                // Check if we have any data
                let has_data = {
                    let trades = TRADES.read();
                    let orderbooks = ORDERBOOKS.read();
                    let stats = CONNECTION_STATS.read();
                    !trades.is_empty() || !orderbooks.is_empty() || !stats.is_empty()
                };

                if has_data {
                    *last_update.write() = Instant::now();
                    has_shown_data = true;
                }

                // Clear screen every 5 iterations to keep it clean
                if iteration % 5 == 0 && has_shown_data {
                    Self::clear_screen();
                }

                Self::print_header();
                Self::print_connection_stats();
                Self::print_trades();
                Self::print_orderbooks();
                Self::print_summary();

                tracing::debug!("\n{}", "=".repeat(120));
                tracing::debug!("Last update: {} | Refresh: {}s | Terminal Monitor Active",
                         chrono::Local::now().format("%H:%M:%S"),
                         refresh_interval.as_secs());

                // Clean up old data periodically
                if iteration % 10 == 0 {
                    Self::cleanup_old_data();
                }
            }
        });

        self.monitor_handle = Some(handle);
        Ok(())
    }

    /// Stop the monitor
    pub fn stop(&mut self) {
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
            tracing::debug!("\nTerminal monitor stopped.");
        }
    }

    /// Print a connection event
    pub fn log_connection_event(exchange: &str, event: &str) {
        if Self::is_terminal_mode() {
            tracing::debug!("[{}] {} WebSocket: {}",
                     chrono::Local::now().format("%H:%M:%S"),
                     exchange,
                     event);
        }
    }

    /// Print a data event
    pub fn log_data_event(exchange: &str, data_type: &str, symbol: &str) {
        if Self::is_terminal_mode() {
            // Only log periodically to avoid spam
            // This is handled by the display loop
        }
    }

    fn is_terminal_mode() -> bool {
        // Check if terminal display is enabled (could be from env var or config)
        std::env::var("TERMINAL_DISPLAY").unwrap_or_else(|_| "true".to_string()) == "true"
    }

    fn clear_screen() {
        print!("\x1B[2J\x1B[1;1H");
    }

    fn print_header() {
        tracing::debug!("\n{}", "=".repeat(120));
        tracing::debug!("{:^120}", "WebSocket Feeder - Active Connections & Market Data");
        tracing::debug!("{}", "=".repeat(120));
    }

    fn print_connection_stats() {
        let stats = CONNECTION_STATS.read();

        if stats.is_empty() {
            tracing::debug!("\n⏳ No WebSocket connections established yet...");
            return;
        }

        tracing::debug!("\n📡 WebSocket Connection Status:");
        tracing::debug!("{:-<120}", "");
        tracing::debug!("{:<15} | {:>10} | {:>12} | {:>10} | {:<50}",
                 "Exchange", "Connected", "Disconnected", "Total", "Status");
        tracing::debug!("{:-<120}", "");

        for (exchange, stat) in stats.iter() {
            let status = if stat.connected > 0 {
                format!("✓ Active ({} connections)", stat.connected)
            } else if stat.disconnected > 0 {
                format!("✗ Disconnected ({})", stat.disconnected)
            } else {
                "⏳ Connecting...".to_string()
            };

            let status_color = if stat.connected > 0 {
                status
            } else {
                status
            };

            tracing::debug!("{:<15} | {:>10} | {:>12} | {:>10} | {:<50}",
                     exchange, stat.connected, stat.disconnected, stat.total_connections, status_color);
        }
    }

    fn print_trades() {
        let trades = TRADES.read();

        if trades.is_empty() {
            return;
        }

        tracing::debug!("\n📈 Recent Trades (Last 5):");
        tracing::debug!("{:-<120}", "");
        tracing::debug!("{:<10} | {:<12} | {:>12} | {:>12} | {:>12} | {:<20}",
                 "Exchange", "Symbol", "Price", "Quantity", "Value", "Time");
        tracing::debug!("{:-<120}", "");

        let recent_trades: Vec<_> = trades.iter().rev().take(5).collect();

        for trade in recent_trades {
            let time = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(trade.timestamp as i64)
                .map(|dt| dt.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "N/A".to_string());

            // Convert scaled i64 to f64 for display
            let price_f64 = trade.get_price_f64();
            let quantity_f64 = trade.get_quantity_f64();
            let value = price_f64 * quantity_f64;

            tracing::debug!("{:<10} | {:<12} | {:>12.4} | {:>12.4} | {:>12.2} | {:<20}",
                     trade.exchange, trade.symbol, price_f64, quantity_f64, value, time);
        }
    }

    fn print_orderbooks() {
        let orderbooks = ORDERBOOKS.read();

        if orderbooks.is_empty() {
            return;
        }

        tracing::debug!("\n📊 Order Books (Last 5):");
        tracing::debug!("{:-<120}", "");
        tracing::debug!("{:<10} | {:<12} | {:>12} | {:>12} | {:>10} | {:>8} | {:>8}",
                 "Exchange", "Symbol", "Bid", "Ask", "Spread(bp)", "BidDepth", "AskDepth");
        tracing::debug!("{:-<120}", "");

        let recent_books: Vec<_> = orderbooks.iter().rev().take(5).collect();

        for ob in recent_books {
            // Convert scaled i64 to f64 for display
            let best_bid_i64 = ob.bids.first().map(|b| b.0).unwrap_or(0);
            let best_ask_i64 = ob.asks.first().map(|a| a.0).unwrap_or(0);
            let best_bid = ob.unscale_price(best_bid_i64);
            let best_ask = ob.unscale_price(best_ask_i64);
            let bid_depth = ob.bids.len();
            let ask_depth = ob.asks.len();

            let spread = if best_bid > 0.0 && best_ask > 0.0 {
                ((best_ask - best_bid) / best_bid) * 10000.0
            } else {
                0.0
            };

            tracing::debug!("{:<10} | {:<12} | {:>12.4} | {:>12.4} | {:>10.1} | {:>8} | {:>8}",
                     ob.exchange, ob.symbol, best_bid, best_ask, spread, bid_depth, ask_depth);
        }
    }

    fn print_summary() {
        let trades = TRADES.read();
        let orderbooks = ORDERBOOKS.read();

        tracing::debug!("\n📊 Data Summary:");
        tracing::debug!("{:-<120}", "");

        let mut trade_counts: HashMap<String, usize> = HashMap::new();
        let mut ob_counts: HashMap<String, usize> = HashMap::new();

        for trade in trades.iter() {
            *trade_counts.entry(trade.exchange.clone()).or_insert(0) += 1;
        }

        for ob in orderbooks.iter() {
            *ob_counts.entry(ob.exchange.clone()).or_insert(0) += 1;
        }

        tracing::debug!("{:<15} | {:>20} | {:>20} | {:>20}",
                 "Exchange", "Trades", "OrderBooks", "Total Updates");
        tracing::debug!("{:-<80}", "");

        let all_exchanges: std::collections::HashSet<_> = trade_counts.keys()
            .chain(ob_counts.keys())
            .collect();

        for exchange in all_exchanges {
            let trade_count = trade_counts.get(exchange).unwrap_or(&0);
            let ob_count = ob_counts.get(exchange).unwrap_or(&0);
            let total = trade_count + ob_count;
            tracing::debug!("{:<15} | {:>20} | {:>20} | {:>20}",
                     exchange, trade_count, ob_count, total);
        }

        tracing::debug!("{:-<80}", "");
        tracing::debug!("{:<15} | {:>20} | {:>20} | {:>20}",
                 "Total", trades.len(), orderbooks.len(), trades.len() + orderbooks.len());
    }

    fn cleanup_old_data() {
        // CircularBuffer automatically limits size, no cleanup needed
    }
}

impl Drop for TerminalMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}