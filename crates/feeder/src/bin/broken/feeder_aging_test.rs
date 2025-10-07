use clap::Parser;
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;
use std::env;
use std::fs::File;
use std::sync::Arc;
use tokio::task::JoinHandle;
use std::time::Duration;

// Feeder imports
use feeder::core::{SymbolMapper, init_global_ordered_udp_sender, get_ordered_udp_sender, init_global_multi_port_sender, get_multi_port_sender, trigger_shutdown, CONNECTION_STATS};
use feeder::core::spawning::*;
use feeder::connect_to_databse::{QuestDBClient, QuestDBConfig};
use tracing::{info, warn, error};

// System metrics
use sysinfo::System;

// Configuration Structs
#[derive(Debug, Deserialize, Clone)]
struct Config {
    test: Test,
    exchanges: Exchanges,
    thresholds: Thresholds,
    questdb: QuestDB,
    monitoring: Monitoring,
}

#[derive(Debug, Deserialize, Clone)]
struct Test {
    test_run_id: String,
    duration_hours: u64,
    metric_collection_interval_sec: u64,
    summary_interval_sec: u64,
}

#[derive(Debug, Deserialize, Clone)]
struct Exchanges {
    enabled: Vec<String>,
}

fn from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<f64>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize, Clone)]
struct Thresholds {
    #[serde(deserialize_with = "from_str")]
    memory_growth_mb_per_hour: f64,
    #[serde(deserialize_with = "from_str")]
    min_uptime_percent: f64,
    max_reconnects_per_hour: u32,
    max_data_gap_seconds: u64,
    #[serde(deserialize_with = "from_str")]
    max_cpu_percent: f64,
}

#[derive(Debug, Deserialize, Clone)]
struct QuestDB {
    host: String,
    ilp_port: u16,
    http_port: u16,
    flush_interval_ms: u64,
}

#[derive(Debug, Deserialize, Clone)]
struct Monitoring {
    enable_terminal_output: bool,
    log_file: String,
    enable_udp_monitor: bool,
}


/// Feeder Aging Test Harness
/// Runs the feeder for extended periods to validate long-term stability.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Duration of the test (e.g., 24h, 3d, 1w)
    #[arg(short, long)]
    duration: Option<String>,

    /// Comma-separated list of exchanges to test
    #[arg(short, long)]
    exchanges: Option<String>,

    /// Path to the configuration file
    #[arg(short, long, default_value = "aging_test_config.toml")]
    config: String,
}


async fn run_feeder_task(config: Config) -> Result<Vec<JoinHandle<()>>> {
    info!("🚀 Spawning feeder task in background...");

    let config_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
        .join("config");

    let symbol_mapper_path = config_dir.join("crypto/symbol_mapping.json");
    if !symbol_mapper_path.exists() {
        return Err(anyhow::anyhow!("symbol_mapping.json not found"));
    }

    let symbol_mapper: SymbolMapper = serde_json::from_reader(File::open(&symbol_mapper_path)?)?;
    let symbol_mapper = Arc::new(symbol_mapper);
    info!("✅ Symbol mapper loaded successfully");

    let mut all_handles = Vec::new();
    let selected_exchanges = config.exchanges.enabled;

    for exchange_name in &selected_exchanges {
        let handle = match exchange_name.as_str() {
            "binance" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/binance_config_full.json").to_str().unwrap())?;
                spawn_binance_exchange(config, symbol_mapper.clone()).await
            },
            "bybit" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/bybit_config_full.json").to_str().unwrap())?;
                spawn_bybit_exchange(config, symbol_mapper.clone()).await
            },
            "upbit" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/upbit_config_full.json").to_str().unwrap())?;
                spawn_upbit_exchange(config, symbol_mapper.clone()).await
            },
            "coinbase" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/coinbase_config_full.json").to_str().unwrap())?;
                spawn_coinbase_exchange(config, symbol_mapper.clone()).await
            },
            "okx" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/okx_config_full.json").to_str().unwrap())?;
                spawn_okx_exchange(config, symbol_mapper.clone()).await
            },
            "deribit" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/deribit_config_full.json").to_str().unwrap())?;
                spawn_deribit_exchange(config, symbol_mapper.clone()).await
            },
            "bithumb" => {
                let config = ExtendedExchangeConfig::load_full_config(config_dir.join("crypto/bithumb_config_full.json").to_str().unwrap())?;
                spawn_bithumb_exchange(config, symbol_mapper.clone()).await
            },
            _ => {
                warn!("Unsupported exchange in aging test: {}", exchange_name);
                continue;
            }
        };
        all_handles.push(handle);
    }

    if all_handles.is_empty() {
        anyhow::bail!("No exchanges were started for the aging test.");
    }

    const MONITOR_ENDPOINT: &str = "239.255.42.99:9001";
    if let Err(e) = init_global_ordered_udp_sender("0.0.0.0:0", MONITOR_ENDPOINT, Some(100), Some(10)).await {
        error!("Failed to initialize UDP sender: {}", e);
        anyhow::bail!("Failed to initialize UDP sender");
    }
    info!("✅ Buffered UDP sender initialized for aging test.");

    // Initialize multi-port UDP sender for proportional load distribution
    if let Err(e) = init_global_multi_port_sender() {
        error!("Failed to initialize multi-port UDP sender: {}", e);
        anyhow::bail!("Failed to initialize multi-port UDP sender");
    }
    info!("✅ Multi-Port UDP sender initialized (20 addresses: Binance 40%, others 10%)");

    info!("✅ Feeder task running with {} exchanges.", all_handles.len());
    Ok(all_handles)
}

async fn run_telemetry_loop(config: Config, questdb_client: Arc<QuestDBClient>, test_run_id: String) {
    info!("🛰️  Starting telemetry collection loop...");
    let mut interval = tokio::time::interval(Duration::from_secs(config.test.metric_collection_interval_sec));
    let mut sys = System::new_all();
    let pid = sysinfo::get_current_pid().unwrap();

    let mut last_total_packets = 0u64;

    loop {
        interval.tick().await;
        sys.refresh_all();

        let now_nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

        // --- System Metrics ---
        if let Some(process) = sys.process(pid) {
            let memory_mb = process.memory() as f64 / 1024.0 / 1024.0;
            let cpu_usage = process.cpu_usage();
            let thread_count = sys.processes().len();

            let mem_line = format!(
                "feeding_aging_test_metrics,test_run_id={},metric_type=resource,metric_name=memory metric_value={:.2},metric_unit=\"MB\" {}",
                test_run_id, memory_mb, now_nanos
            );
            let cpu_line = format!(
                "feeding_aging_test_metrics,test_run_id={},metric_type=resource,metric_name=cpu metric_value={:.2},metric_unit=\"%%\" {}",
                test_run_id, cpu_usage, now_nanos
            );
            let thread_line = format!(
                "feeding_aging_test_metrics,test_run_id={},metric_type=resource,metric_name=threads metric_value={}i,metric_unit=\"count\" {}",
                test_run_id, thread_count, now_nanos
            );

            if let Err(e) = questdb_client.write_ilp_line(&mem_line).await {
                 warn!("Failed to send memory metric to QuestDB: {}", e);
            }
            if let Err(e) = questdb_client.write_ilp_line(&cpu_line).await {
                 warn!("Failed to send cpu metric to QuestDB: {}", e);
            }
            if let Err(e) = questdb_client.write_ilp_line(&thread_line).await {
                 warn!("Failed to send thread metric to QuestDB: {}", e);
            }
        }

        // --- Connection Metrics ---
        let reconnect_lines: Vec<String> = {
            let conn_stats = CONNECTION_STATS.read();
            conn_stats.iter().map(|(exchange, stats)| {
                format!(
                    "feeding_aging_test_metrics,test_run_id={},exchange={},metric_type=connection,metric_name=reconnects metric_value={}i,metric_unit=\"count\" {}",
                    test_run_id, exchange, stats.reconnect_count, now_nanos
                )
            }).collect()
        };

        for reconnect_line in reconnect_lines {
             if let Err(e) = questdb_client.write_ilp_line(&reconnect_line).await {
                 warn!("Failed to send reconnect metric to QuestDB: {}", e);
            }
        }

        // --- Data Rate Metrics ---
        if let Some(sender) = get_ordered_udp_sender() {
            let total_packets = sender.get_total_packets_sent();
            let packets_in_interval = total_packets.saturating_sub(last_total_packets);
            let data_rate_pps = packets_in_interval as f64 / config.test.metric_collection_interval_sec as f64;
            last_total_packets = total_packets;

            let rate_line = format!(
                "feeding_aging_test_metrics,test_run_id={},metric_type=data_quality,metric_name=packet_rate metric_value={:.2},metric_unit=\"pps\" {}",
                test_run_id, data_rate_pps, now_nanos
            );
            if let Err(e) = questdb_client.write_ilp_line(&rate_line).await {
                 warn!("Failed to send data rate metric to QuestDB: {}", e);
            }
        }

        info!("Telemetry tick completed. Metrics sent to QuestDB.");
    }
}

async fn generate_summary_report(questdb_client: Arc<QuestDBClient>, test_run_id: &str, config: &Config) -> Result<()> {
    // Queries to calculate aggregates
    let avg_cpu_query = format!(
        "SELECT avg(metric_value) FROM feeding_aging_test_metrics WHERE test_run_id = '{}' AND metric_name = 'cpu'",
        test_run_id
    );
    let mem_query = format!(
        "SELECT first(metric_value), last(metric_value) FROM feeding_aging_test_metrics WHERE test_run_id = '{}' AND metric_name = 'memory'",
        test_run_id
    );
    let total_reconnects_query = format!(
        "SELECT sum(metric_value) FROM feeding_aging_test_metrics WHERE test_run_id = '{}' AND metric_name = 'reconnects'",
        test_run_id
    );

    // Execute queries
    let avg_cpu_response = questdb_client.query(&avg_cpu_query).await?;
    let mem_response = questdb_client.query(&mem_query).await?;
    let reconnects_response = questdb_client.query(&total_reconnects_query).await?;

    // --- Parse responses (basic CSV parsing) ---
    let avg_cpu = avg_cpu_response.lines().nth(1).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    
    let (initial_mem, final_mem) = mem_response.lines().nth(1).map(|line| {
        let mut parts = line.split(',');
        (parts.next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
         parts.next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0))
    }).unwrap_or((0.0, 0.0));
    let mem_growth = final_mem - initial_mem;

    let total_reconnects = reconnects_response.lines().nth(1).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);

    // --- Print Console Report ---
    println!("\n═══════════════════════════════════════════════════════════");
    println!("         FEEDER AGING TEST FINAL REPORT");
    println!("═══════════════════════════════════════════════════════════");
    println!("Test Run ID: {}", test_run_id);
    println!("Duration: {} hours", config.test.duration_hours);
    
    println!("\nMEMORY MANAGEMENT");
    println!("─────────────────────────────────────────────────────────");
    println!("Initial Memory:     {:.2} MB", initial_mem);
    println!("Final Memory:       {:.2} MB", final_mem);
    println!("Total Growth:       {:.2} MB", mem_growth);

    println!("\nRESOURCE UTILIZATION");
    println!("─────────────────────────────────────────────────────────");
    println!("CPU Average:        {:.2}%", avg_cpu);

    println!("\nCONNECTION STABILITY");
    println!("─────────────────────────────────────────────────────────");
    println!("Total Reconnects:   {}", total_reconnects);
    println!("═══════════════════════════════════════════════════════════\n");

    // --- Write summary to QuestDB ---
    let summary_line = format!(
        "feeding_aging_test_summary,test_run_id={} test_duration_hours={},total_reconnects={}i,memory_leaked_mb={},cpu_avg_percent={} {}",
        test_run_id,
        config.test.duration_hours,
        total_reconnects,
        mem_growth,
        avg_cpu,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );

    questdb_client.write_ilp_line(&summary_line).await?;
    info!("✅ Final summary report written to QuestDB.");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!("Starting feeder aging test...");
    
    let config_path = PathBuf::from(&args.config);
    if !config_path.exists() {
        anyhow::bail!("Configuration file not found: {}", args.config);
    }

    let settings = config::Config::builder()
        .add_source(config::File::from(config_path))
        .build()?;
        
    let config: Config = settings.try_deserialize()?;

    // --- Initialize QuestDB client ---
    let test_run_id = if config.test.test_run_id == "auto" {
        format!("feeding_aging_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S"))
    } else {
        config.test.test_run_id.clone()
    };
    info!("Generated Test Run ID: {}", test_run_id);

    let questdb_conf = QuestDBConfig {
        host: config.questdb.host.clone(),
        ilp_port: config.questdb.ilp_port,
        http_port: config.questdb.http_port,
        ..Default::default()
    };
    let questdb_client = Arc::new(QuestDBClient::new(questdb_conf).await?);
    info!("✅ QuestDB client initialized.");

    println!("✅ Configuration loaded successfully from '{}'", args.config);
    
    // --- Spawn feeder task ---
    let feeder_handles = run_feeder_task(config.clone()).await?;

    // --- Spawn telemetry loop ---
    let telemetry_handle = tokio::spawn(run_telemetry_loop(config.clone(), questdb_client.clone(), test_run_id.clone()));

    // --- Wait for the duration of the test ---
    let duration_secs = config.test.duration_hours * 3600;
    info!("Aging test will run for {} hours. Press Ctrl+C to stop early.", config.test.duration_hours);
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(duration_secs)) => {
            info!("Test duration completed.");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C received, stopping test early.");
        }
    }

    // --- Step 7: Generate and write the final summary report ---
    info!("Generating final summary report...");
    if let Err(e) = generate_summary_report(questdb_client.clone(), &test_run_id, &config).await {
        error!("Failed to generate summary report: {}", e);
    }

    // Cleanly shut down tasks
    trigger_shutdown();
    telemetry_handle.abort();
    for handle in feeder_handles {
        handle.abort();
    }
    println!("Feeder aging test finished.");

    Ok(())
}