use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use tokio::time::interval;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use serde_json;
use feeder::core::{TradeData, OrderBookData};

struct App {
    should_quit: bool,
    last_update: std::time::Instant,
    connection_data: HashMap<String, ConnectionRow>,
    exchange_data: HashMap<String, ExchangeData>,
    total_packets: usize,
}

#[derive(Clone, Default)]
struct ExchangeData {
    trades: usize,
    orderbooks: usize,
    last_update: Option<Instant>,
}

#[derive(Clone, Default)]
struct ConnectionRow {
    connected: usize,
    disconnected: usize,
    reconnect_count: usize,
    total: usize,
}

#[derive(Debug, Clone)]
enum UdpMessage {
    Exchange { exchange: String, asset_type: String, trades: usize, orderbooks: usize },
    Connection { exchange: String, connected: usize, disconnected: usize, reconnect_count: usize, total: usize },
    Stats { total_trades: usize, total_orderbooks: usize },
    Test(String),
}

impl App {
    fn new() -> Self {
        Self {
            should_quit: false,
            last_update: std::time::Instant::now(),
            connection_data: HashMap::new(),
            exchange_data: HashMap::new(),
            total_packets: 0,
        }
    }

    fn process_message(&mut self, msg: UdpMessage) {
        self.total_packets += 1;
        self.last_update = Instant::now();
        
        match msg {
            UdpMessage::Exchange { exchange, asset_type, trades, orderbooks } => {
                let key = format!("{}_{}", exchange, asset_type);
                let data = self.exchange_data.entry(key).or_default();
                // Accumulate counts instead of replacing
                data.trades += trades;
                data.orderbooks += orderbooks;
                data.last_update = Some(Instant::now());
            }
            UdpMessage::Connection { exchange, connected, disconnected, reconnect_count, total } => {
                let conn = self.connection_data.entry(exchange).or_default();
                conn.connected = connected;
                conn.disconnected = disconnected;
                conn.reconnect_count = reconnect_count;
                conn.total = total;
            }
            UdpMessage::Stats { .. } => {
                // Legacy format, handled by Exchange messages now
            }
            UdpMessage::Test(_) => {
                // Test packet received
            }
        }
    }

    fn on_key(&mut self, c: char) {
        match c {
            'q' => self.should_quit = true,
            _ => {}
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),   // Title
            Constraint::Length(8),   // Connection Status
            Constraint::Min(10),     // Exchange Data
            Constraint::Length(5),   // Summary
        ])
        .split(f.size());

    // Title
    let title = Paragraph::new("UDP Monitor - Real-time Market Data (Press 'q' to quit)")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Connection Status
    draw_connections(f, chunks[1], &app.connection_data);
    
    // Exchange Data
    draw_exchange_data(f, chunks[2], &app.exchange_data);
    
    // Summary
    draw_summary(f, chunks[3], app.total_packets, app.last_update);
}

fn draw_connections(f: &mut Frame, area: Rect, data: &HashMap<String, ConnectionRow>) {
    let header_cells = ["Exchange", "Connected", "Disconnected", "Reconnects", "Total", "Status"]
        .iter()
        .map(|h| ratatui::widgets::Cell::from(*h).style(Style::default().fg(Color::Yellow)));
    let header = Row::new(header_cells)
        .style(Style::default().add_modifier(Modifier::BOLD))
        .height(1);

    let rows = data.iter().map(|(exchange, conn)| {
        let status = if conn.connected > 0 {
            format!("✓ Active ({})", conn.connected)
        } else if conn.disconnected > 0 {
            "✗ Disconnected".to_string()
        } else {
            "⏳ Connecting".to_string()
        };
        
        let status_color = if conn.connected > 0 {
            Color::Green
        } else if conn.disconnected > 0 {
            Color::Red
        } else {
            Color::Yellow
        };
        
        let cells = vec![
            ratatui::widgets::Cell::from(exchange.clone()),
            ratatui::widgets::Cell::from(conn.connected.to_string()).style(Style::default().fg(Color::Green)),
            ratatui::widgets::Cell::from(conn.disconnected.to_string()).style(Style::default().fg(Color::Red)),
            ratatui::widgets::Cell::from(conn.reconnect_count.to_string()).style(Style::default().fg(Color::Cyan)),
            ratatui::widgets::Cell::from(conn.total.to_string()),
            ratatui::widgets::Cell::from(status).style(Style::default().fg(status_color)),
        ];
        Row::new(cells).height(1)
    });

    let widths = [
        Constraint::Percentage(20),  // Exchange
        Constraint::Percentage(15),  // Connected
        Constraint::Percentage(18),  // Disconnected
        Constraint::Percentage(15),  // Reconnects (NEW)
        Constraint::Percentage(12),  // Total
        Constraint::Percentage(20),  // Status
    ];
    
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("📡 WebSocket Connections"));

    f.render_widget(table, area);
}

fn draw_exchange_data(f: &mut Frame, area: Rect, data: &HashMap<String, ExchangeData>) {
    let header_cells = ["Exchange/Type", "Trades", "OrderBooks", "Status", "Last Update"]
        .iter()
        .map(|h| ratatui::widgets::Cell::from(*h).style(Style::default().fg(Color::Yellow)));
    let header = Row::new(header_cells)
        .style(Style::default().add_modifier(Modifier::BOLD))
        .height(1);

    let rows = data.iter().map(|(key, stats)| {
        let age = stats.last_update
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(999);
        
        let status_color = if age < 5 {
            Color::Green
        } else if age < 30 {
            Color::Yellow
        } else {
            Color::Red
        };
        
        let status = if age < 5 {
            "● Active"
        } else if age < 30 {
            "● Slow"
        } else {
            "● Stale"
        };
        
        let cells = vec![
            ratatui::widgets::Cell::from(key.clone()),
            ratatui::widgets::Cell::from(stats.trades.to_string()).style(Style::default().fg(Color::Cyan)),
            ratatui::widgets::Cell::from(stats.orderbooks.to_string()).style(Style::default().fg(Color::Magenta)),
            ratatui::widgets::Cell::from(status).style(Style::default().fg(status_color)),
            ratatui::widgets::Cell::from(format!("{}s ago", age)),
        ];
        Row::new(cells).height(1)
    });

    let widths = [
        Constraint::Percentage(30),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
    ];
    
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("📊 Exchange Data"));

    f.render_widget(table, area);
}

fn draw_summary(f: &mut Frame, area: Rect, total_packets: usize, last_update: std::time::Instant) {
    let elapsed = last_update.elapsed().as_secs();
    let status_color = if elapsed < 5 {
        Color::Green
    } else if elapsed < 30 {
        Color::Yellow
    } else {
        Color::Red
    };
    
    let text = vec![
        Line::from(vec![
            Span::raw("Total Packets: "),
            Span::styled(total_packets.to_string(), Style::default().fg(Color::Cyan)),
            Span::raw("  |  Last Update: "),
            Span::styled(format!("{}s ago", elapsed), Style::default().fg(status_color)),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Summary"))
        .alignment(Alignment::Center);
    
    f.render_widget(paragraph, area);
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut msg_rx: mpsc::Receiver<UdpMessage>) -> io::Result<()> {
    let mut app = App::new();
    let mut update_ticker = interval(Duration::from_millis(100));

    loop {
        // Process UDP messages (batch receive)
        let mut messages_processed = 0;
        while let Ok(msg) = msg_rx.try_recv() {
            app.process_message(msg);
            messages_processed += 1;
            if messages_processed > 100 { // Limit batch size
                break;
            }
        }
        
        // Draw UI
        terminal.draw(|f| draw(f, &app))?;

        // Handle events
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Char(c) => app.on_key(c),
                    _ => {}
                }
            }
        }

        update_ticker.tick().await;
        
        if app.should_quit {
            return Ok(());
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create channel for UDP messages
    let (msg_tx, msg_rx) = mpsc::channel::<UdpMessage>(1000);
    
    // Start UDP listener
    let udp_handle = tokio::spawn(async move {
        let socket = match UdpSocket::bind("0.0.0.0:9001").await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to bind UDP socket: {}", e);
                return;
            }
        };
        
        // Join the multicast group 239.1.1.1
        let multicast_addr = "239.1.1.1".parse::<std::net::Ipv4Addr>().unwrap();
        let interface_addr = "0.0.0.0".parse::<std::net::Ipv4Addr>().unwrap();
        if let Err(e) = socket.join_multicast_v4(multicast_addr, interface_addr) {
            eprintln!("Failed to join multicast group: {}", e);
            // Continue anyway - might still work for unicast
        } else {
            eprintln!("Successfully joined multicast group 239.1.1.1:9001");
        }
        
        let mut buf = vec![0u8; 65536]; // Increased buffer size to handle larger packets
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, _addr)) => {
                    let data = String::from_utf8_lossy(&buf[..len]);
                    let parts: Vec<&str> = data.split('|').collect();
                    
                    let msg = match parts[0] {
                        "TRADE" if parts.len() >= 2 => {
                            // Parse JSON trade data
                            if let Ok(trade) = serde_json::from_str::<TradeData>(parts[1]) {
                                Some(UdpMessage::Exchange {
                                    exchange: trade.exchange,
                                    asset_type: trade.asset_type,
                                    trades: 1,
                                    orderbooks: 0,
                                })
                            } else {
                                None
                            }
                        }
                        "ORDERBOOK" if parts.len() >= 2 => {
                            // Parse JSON orderbook data
                            if let Ok(orderbook) = serde_json::from_str::<OrderBookData>(parts[1]) {
                                Some(UdpMessage::Exchange {
                                    exchange: orderbook.exchange,
                                    asset_type: orderbook.asset_type,
                                    trades: 0,
                                    orderbooks: 1,
                                })
                            } else {
                                None
                            }
                        }
                        "EXCHANGE" if parts.len() >= 6 => {
                            Some(UdpMessage::Exchange {
                                exchange: parts[1].to_string(),
                                asset_type: parts[2].to_string(),
                                trades: parts[3].parse().unwrap_or(0),
                                orderbooks: parts[4].parse().unwrap_or(0),
                            })
                        }
                        "CONN" if parts.len() >= 6 => {
                            Some(UdpMessage::Connection {
                                exchange: parts[1].to_string(),
                                connected: parts[2].parse().unwrap_or(0),
                                disconnected: parts[3].parse().unwrap_or(0),
                                reconnect_count: parts[4].parse().unwrap_or(0),
                                total: parts[5].parse().unwrap_or(0),
                            })
                        }
                        "STATS" if parts.len() >= 3 => {
                            Some(UdpMessage::Stats {
                                total_trades: parts[1].parse().unwrap_or(0),
                                total_orderbooks: parts[2].parse().unwrap_or(0),
                            })
                        }
                        "TEST" => Some(UdpMessage::Test(data.to_string())),
                        _ => None,
                    };
                    
                    if let Some(msg) = msg {
                        let _ = msg_tx.send(msg).await;
                    }
                }
                Err(e) => {
                    eprintln!("Error receiving UDP: {}", e);
                }
            }
        }
    });
    
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let res = run_app(&mut terminal, msg_rx).await;

    // Cleanup
    udp_handle.abort();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}