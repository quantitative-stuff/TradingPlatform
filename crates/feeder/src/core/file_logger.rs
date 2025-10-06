// File-based logging configuration for silent operation
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use chrono::Local;
use tracing::info;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::prelude::*;

pub struct FileLogger {
    log_dir: PathBuf,
    max_file_size: u64,
    max_files: usize,
}

impl FileLogger {
    pub fn new() -> Self {
        Self {
            log_dir: PathBuf::from("logs"),
            max_file_size: 10 * 1024 * 1024, // 10MB per file
            max_files: 10, // Keep last 10 log files
        }
    }
    
    pub fn init(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create logs directory if it doesn't exist
        fs::create_dir_all(&self.log_dir)?;
        
        // Generate log file name with timestamp
        let log_file = self.log_dir.join(format!(
            "feeder_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ));
        
        // Open file for writing
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_file)?;
        
        // Set up file layer for tracing (use try_init to avoid panic if already initialized)
        let _ = tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false) // No color codes in file
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_line_number(true)
            .with_max_level(tracing::Level::INFO)
            .try_init();
        
        // Write startup message
        info!("=== Feeder started - Logging to {} ===", log_file.display());
        
        // Clean up old log files
        self.cleanup_old_logs()?;
        
        Ok(())
    }

    pub fn init_with_questdb(&self, questdb: Option<Arc<RwLock<crate::connect_to_databse::QuestDBClient>>>) -> Result<(), Box<dyn std::error::Error>> {
        // Create logs directory if it doesn't exist
        fs::create_dir_all(&self.log_dir)?;
        
        // Generate log file name with timestamp
        let log_file = self.log_dir.join(format!(
            "feeder_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ));
        
        // Open file for writing
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_file)?;
        
        // Set up file layer
        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(file)
            .with_ansi(false) // No color codes in file
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_line_number(true);
        
        // Build subscriber with optional QuestDB layer
        let registry = tracing_subscriber::registry()
            .with(file_layer)
            .with(tracing_subscriber::filter::LevelFilter::INFO);
        
        let _ = if let Some(questdb) = questdb {
            let questdb_layer = crate::connect_to_databse::QuestDBLogLayer::new(questdb);
            registry.with(questdb_layer).try_init()
        } else {
            registry.try_init()
        };
        
        // Write startup message
        info!("=== Feeder started - Logging to {} ===", log_file.display());
        
        // Clean up old log files
        self.cleanup_old_logs()?;
        
        Ok(())
    }
    
    pub fn init_silent(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Silent mode - only log errors and warnings to file
        fs::create_dir_all(&self.log_dir)?;
        
        let log_file = self.log_dir.join(format!(
            "feeder_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ));
        
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_file)?;
        
        // Only log warn and error levels in silent mode
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
            .with_target(false)
            .with_thread_ids(false)
            .with_line_number(true)
            .with_max_level(tracing::Level::WARN)
            .init();
        
        // Create a simple status file for monitoring
        let status_file = self.log_dir.join("feeder_status.txt");
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(status_file)?;
        
        writeln!(file, "Feeder started at: {}", Local::now())?;
        writeln!(file, "Log file: {}", log_file.display())?;
        writeln!(file, "Mode: SILENT (only warnings/errors logged)")?;
        
        Ok(())
    }
    
    fn cleanup_old_logs(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut log_files: Vec<_> = fs::read_dir(&self.log_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name()
                    .to_string_lossy()
                    .starts_with("feeder_")
                    && entry.file_name()
                    .to_string_lossy()
                    .ends_with(".log")
            })
            .collect();
        
        // Sort by modification time (oldest first)
        log_files.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        
        // Remove old files if we have too many
        while log_files.len() > self.max_files {
            if let Some(old_file) = log_files.first() {
                fs::remove_file(old_file.path())?;
                log_files.remove(0);
            }
        }
        
        Ok(())
    }
}

// Redirect stdout/stderr to suppress all console output
pub fn suppress_console_output() {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            let devnull = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/null")
                .unwrap();
            libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO);
            libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
        }
    }
    
    #[cfg(windows)]
    {
        // Windows doesn't have /dev/null, but we can redirect to NUL
        
        // This is more complex on Windows, so we'll just suppress prints in the code
    }
}

// Helper macro to replace println! when in silent mode
#[macro_export]
macro_rules! log_print {
    ($($arg:tt)*) => {
        if std::env::var("SILENT_MODE").unwrap_or_default() != "true" {
            println!($($arg)*);
        } else {
            tracing::info!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_eprint {
    ($($arg:tt)*) => {
        if std::env::var("SILENT_MODE").unwrap_or_default() != "true" {
            eprintln!($($arg)*);
        } else {
            tracing::error!($($arg)*);
        }
    };
}