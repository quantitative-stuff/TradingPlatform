use tracing::{Level, info};
use std::fs;
use std::path::Path;

pub struct LoggingConfig {
    pub log_level: String,
    pub log_file: String,
    pub max_log_size_mb: u64,
    pub log_rotation: String,
}

pub fn setup_logging(config: &LoggingConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Create log directory if it doesn't exist
    if let Some(log_dir) = Path::new(&config.log_file).parent() {
        fs::create_dir_all(log_dir)?;
    }

    // Parse log level
    let level = match config.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    // Initialize basic console logging (use try_init to avoid panic if already initialized)
    let _ = tracing_subscriber::fmt()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_max_level(level)
        .try_init();

    info!("Logging initialized with level: {}", config.log_level);
    info!("Log file: {}", config.log_file);

    Ok(())
}

pub fn setup_console_logging(level: &str) -> Result<(), Box<dyn std::error::Error>> {
    let level = match level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    let _ = tracing_subscriber::fmt()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_max_level(level)
        .try_init();

    info!("Console logging initialized with level: {}", level);

    Ok(())
} 