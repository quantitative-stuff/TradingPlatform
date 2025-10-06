/// Demo: Multi-port UDP sender
/// Shows how symbols are distributed across multiple ports for better performance

use feeder::core::{
    MultiPortUdpSender, MultiPortConfig, PartitionStrategy,
};
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== Multi-Port UDP Sender Demo ===");
    println!();

    // Test different strategies
    demo_symbol_hash();
    println!();
    demo_exchange_symbol_hash();
    println!();
    demo_round_robin();
}

fn demo_symbol_hash() {
    println!("=== Strategy 1: Symbol Hash ===");
    println!("Same symbol always goes to same port (good for consistent ordering)");
    println!();

    let config = MultiPortConfig {
        base_address: "239.0.0.1".to_string(),
        base_port: 9001,
        num_ports: 4,
        strategy: PartitionStrategy::SymbolHash,
    };

    let sender = MultiPortUdpSender::new(config).unwrap();

    // Send some test data
    let symbols = vec!["BTC/USDT", "ETH/USDT", "SOL/USDT", "DOGE/USDT", "XRP/USDT"];
    let exchanges = vec!["binance", "bybit"];

    for symbol in &symbols {
        for exchange in &exchanges {
            let _ = sender.send_trade(
                exchange,
                symbol,
                50000.0,
                1.5,
                true,
                1000000000,
            );
        }
    }

    sender.print_stats();
    println!("Note: Same symbol on different exchanges goes to SAME port");
}

fn demo_exchange_symbol_hash() {
    println!("=== Strategy 2: Exchange+Symbol Hash ===");
    println!("Symbol+Exchange combo determines port (better distribution)");
    println!();

    let config = MultiPortConfig {
        base_address: "239.0.0.1".to_string(),
        base_port: 9001,
        num_ports: 4,
        strategy: PartitionStrategy::ExchangeSymbolHash,
    };

    let sender = MultiPortUdpSender::new(config).unwrap();

    // Send some test data
    let symbols = vec!["BTC/USDT", "ETH/USDT", "SOL/USDT", "DOGE/USDT", "XRP/USDT"];
    let exchanges = vec!["binance", "bybit"];

    for symbol in &symbols {
        for exchange in &exchanges {
            let _ = sender.send_trade(
                exchange,
                symbol,
                50000.0,
                1.5,
                true,
                1000000000,
            );
        }
    }

    sender.print_stats();
    println!("Note: Same symbol on different exchanges may go to DIFFERENT ports");
}

fn demo_round_robin() {
    println!("=== Strategy 3: Round Robin ===");
    println!("Each packet goes to next port (simple, even distribution)");
    println!();

    let config = MultiPortConfig {
        base_address: "239.0.0.1".to_string(),
        base_port: 9001,
        num_ports: 4,
        strategy: PartitionStrategy::RoundRobin,
    };

    let sender = MultiPortUdpSender::new(config).unwrap();

    // Send some test data
    let symbols = vec!["BTC/USDT", "ETH/USDT", "SOL/USDT", "DOGE/USDT"];
    let exchanges = vec!["binance", "bybit"];

    for symbol in &symbols {
        for exchange in &exchanges {
            let _ = sender.send_trade(
                exchange,
                symbol,
                50000.0,
                1.5,
                true,
                1000000000,
            );
        }
    }

    sender.print_stats();
    println!("Note: Perfect distribution but ordering not guaranteed");
}

fn _demo_continuous_sending() {
    println!("=== Continuous Sending Demo ===");
    println!("Sending 1000 packets...");

    let config = MultiPortConfig {
        base_address: "239.0.0.1".to_string(),
        base_port: 9001,
        num_ports: 4,
        strategy: PartitionStrategy::SymbolHash,
    };

    let sender = MultiPortUdpSender::new(config).unwrap();

    let symbols = vec![
        "BTC/USDT", "ETH/USDT", "SOL/USDT", "DOGE/USDT",
        "XRP/USDT", "ADA/USDT", "DOT/USDT", "MATIC/USDT",
    ];
    let exchanges = vec!["binance", "bybit", "upbit", "okx"];

    for i in 0..1000 {
        let symbol = symbols[i % symbols.len()];
        let exchange = exchanges[i % exchanges.len()];
        let price = 50000.0 + (i as f64 * 0.1);

        let _ = sender.send_trade(
            exchange,
            symbol,
            price,
            1.5,
            i % 2 == 0,
            1000000000 + i as u64,
        );

        if i % 100 == 0 && i > 0 {
            print!(".");
            std::io::Write::flush(&mut std::io::stdout()).ok();
        }
    }

    println!("\n");
    sender.print_stats();
}

fn _demo_with_receiver() {
    println!("=== Multi-Port with Receiver Demo ===");
    println!("1. Start this sender");
    println!("2. In another terminal, run: cargo run --bin multi_port_receiver");
    println!();
    println!("Sending to ports 9001-9004 for 30 seconds...");

    let config = MultiPortConfig {
        base_address: "239.0.0.1".to_string(),
        base_port: 9001,
        num_ports: 4,
        strategy: PartitionStrategy::SymbolHash,
    };

    let sender = MultiPortUdpSender::new(config).unwrap();

    let symbols = vec!["BTC/USDT", "ETH/USDT", "SOL/USDT"];
    let exchanges = vec!["binance", "bybit"];

    for i in 0..300 {
        let symbol = symbols[i % symbols.len()];
        let exchange = exchanges[i % exchanges.len()];

        // Send orderbook
        let bids = vec![(50000.0, 1.0), (49999.0, 2.0)];
        let asks = vec![(50001.0, 1.5), (50002.0, 2.5)];

        let _ = sender.send_orderbook(
            exchange,
            symbol,
            &bids,
            &asks,
            1000000000 + i as u64,
        );

        thread::sleep(Duration::from_millis(100));

        if i % 10 == 0 {
            println!("[{}s] Sent {} updates", i / 10, i);
        }
    }

    sender.print_stats();
}