use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Invalid input: {0}")]
    InvalidInput(String),  // Add this variant
    
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Parse error: {0}")]
    ParseError(String),  // Add this variant

    #[error("Config error: {0}")]
    Config(String),

    // #[error("ClickHouse error: {0}")]
    // ClickHouse(#[from] clickhouse::error::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Channel error: {0}")]
    Channel(String),
}

pub type Result<T> = std::result::Result<T, Error>;
