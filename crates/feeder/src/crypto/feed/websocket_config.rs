use tokio_tungstenite::{
    WebSocketStream, 
    MaybeTlsStream,
    tungstenite::{
        protocol::WebSocketConfig,
        client::IntoClientRequest,
        http::Response,
    },
    connect_async_with_config,
};
use tokio::net::TcpStream;
use crate::error::Result;

/// Create a WebSocket connection with optimized settings for receiving large data
pub async fn connect_with_large_buffer(
    url: &str
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response<()>)> {
    
    // Configure WebSocket with larger buffers for exchanges that send large data
    // WebSocketConfig is non-exhaustive, so we need to build it differently
    let mut config = WebSocketConfig::default();
    
    // Set larger message and frame sizes for receiving exchange data
    config.max_message_size = Some(64 * 1024 * 1024);  // 64 MB (from default 64KB)
    config.max_frame_size = Some(16 * 1024 * 1024);    // 16 MB
    
    // Increase buffer sizes for better performance with large data streams
    config.read_buffer_size = 64 * 1024;               // 64 KB read buffer
    config.write_buffer_size = 64 * 1024;              // 64 KB write buffer
    config.max_write_buffer_size = 256 * 1024;         // 256 KB max write buffer
    
    let request = url.into_client_request()?;
    
    match connect_async_with_config(request, Some(config), false).await {
        Ok((ws_stream, response)) => {
            // Convert Response<Option<Vec<u8>>> to Response<()>
            let converted_response = response.map(|_| ());
            Ok((ws_stream, converted_response))
        },
        Err(e) => Err(crate::error::Error::Connection(format!("WebSocket connection failed: {}", e)))
    }
}

/// Connect with default settings (for backwards compatibility)
pub async fn connect_default(
    url: &str
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response<()>)> {
    match tokio_tungstenite::connect_async(url).await {
        Ok((ws_stream, response)) => {
            // Convert Response<Option<Vec<u8>>> to Response<()>
            let converted_response = response.map(|_| ());
            Ok((ws_stream, converted_response))
        },
        Err(e) => Err(crate::error::Error::Connection(format!("WebSocket connection failed: {}", e)))
    }
}