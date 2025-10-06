/// Safe logging utilities to prevent UDP packet size limits (OS Error 10040)
/// 
/// These utilities ensure that log messages fit within QuestDB UDP transmission limits
/// while preserving the most important debugging information.

/// Maximum safe length for a single log message field to prevent UDP overflow
const MAX_LOG_MESSAGE_LENGTH: usize = 400;

/// Maximum length for JSON/text data in logs
const MAX_JSON_LOG_LENGTH: usize = 200;

/// Maximum length for error messages in logs  
const MAX_ERROR_LOG_LENGTH: usize = 300;

/// Safely truncate a string for logging, preserving important information
pub fn truncate_for_log(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...[TRUNCATED_{}chars]", &text[..max_len.saturating_sub(25)], text.len())
    }
}

/// Safely log JSON data with size limits
pub fn safe_json_log(json_text: &str) -> String {
    truncate_for_log(json_text, MAX_JSON_LOG_LENGTH)
}

/// Safely log error messages with size limits
pub fn safe_error_log(error_msg: &str) -> String {
    truncate_for_log(error_msg, MAX_ERROR_LOG_LENGTH)
}

/// Safely log general messages with size limits
pub fn safe_message_log(message: &str) -> String {
    truncate_for_log(message, MAX_LOG_MESSAGE_LENGTH)
}

/// Safely format JSON parsing errors for logging
pub fn safe_json_parse_error_log(error: &dyn std::fmt::Display, raw_text: &str) -> String {
    let error_str = error.to_string();
    let safe_error = truncate_for_log(&error_str, 100);
    let safe_json = safe_json_log(raw_text);
    format!("JSON parse error: {}. Raw text: {}", safe_error, safe_json)
}

/// Safely format connection errors for logging
pub fn safe_connection_error_log(error: &dyn std::fmt::Display, context: &str) -> String {
    let error_str = error.to_string();
    let safe_error = safe_error_log(&error_str);
    let safe_context = truncate_for_log(context, 100);
    format!("Connection error [{}]: {}", safe_context, safe_error)
}

/// Safely format WebSocket message for logging
pub fn safe_websocket_message_log(exchange: &str, message: &str) -> String {
    let safe_message = safe_json_log(message);
    format!("[{}] Message received: {}", exchange, safe_message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_string() {
        let short = "short message";
        assert_eq!(truncate_for_log(short, 100), short);
    }

    #[test]
    fn test_truncate_long_string() {
        let long = "a".repeat(500);
        let result = truncate_for_log(&long, 100);
        assert!(result.len() <= 100);
        assert!(result.contains("TRUNCATED_500chars"));
    }

    #[test]
    fn test_safe_json_log() {
        let long_json = r#"{"very":"long","json":"object","with":{"nested":"data","that":"goes","on":"and","on":"with","lots":"of","fields"}}"#.repeat(5);
        let result = safe_json_log(&long_json);
        assert!(result.len() <= MAX_JSON_LOG_LENGTH);
    }

    #[test]
    fn test_safe_json_parse_error() {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid JSON format");
        let long_json = r#"{"invalid":"json","that":"is","very":"long"}"#.repeat(10);
        let result = safe_json_parse_error_log(&error, &long_json);
        assert!(result.len() <= 350); // Should fit in a reasonable UDP packet
    }
}