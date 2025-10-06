use std::collections::HashMap;
use once_cell::sync::Lazy;
use tracing::{warn, info};

/// Timestamp normalization to ensure all timestamps are in milliseconds
/// Handles different formats from various exchanges

/// Known exchange timestamp formats
static EXCHANGE_FORMATS: Lazy<HashMap<&'static str, TimestampFormat>> = Lazy::new(|| {
    let mut formats = HashMap::new();
    
    // Most exchanges use milliseconds (13 digits)
    formats.insert("Binance", TimestampFormat::Milliseconds);
    formats.insert("Bybit", TimestampFormat::Milliseconds);
    formats.insert("OKX", TimestampFormat::Milliseconds);
    formats.insert("Coinbase", TimestampFormat::Milliseconds);
    
    // Some exchanges use seconds (10 digits)
    formats.insert("Upbit", TimestampFormat::Seconds);
    formats.insert("Bithumb", TimestampFormat::Seconds);
    
    // Deribit uses microseconds (16 digits)
    formats.insert("Deribit", TimestampFormat::Microseconds);
    
    formats
});

#[derive(Debug, Clone, Copy)]
pub enum TimestampFormat {
    Seconds,        // 10 digits - multiply by 1000
    Milliseconds,   // 13 digits - standard, no conversion
    Microseconds,   // 16 digits - divide by 1000
    Nanoseconds,    // 19 digits - divide by 1000000
}

/// Normalize any timestamp to milliseconds
pub fn normalize_timestamp(timestamp: i64, exchange: &str) -> i64 {
    // First try to auto-detect based on digit count
    let digit_count = if timestamp == 0 {
        return 0;
    } else {
        timestamp.abs().to_string().len()
    };
    
    // Check if we have a known format for this exchange
    if let Some(&format) = EXCHANGE_FORMATS.get(exchange) {
        return convert_by_format(timestamp, format, exchange);
    }
    
    // Auto-detect based on digit count
    let detected_format = match digit_count {
        1..=10 => TimestampFormat::Seconds,
        11..=13 => TimestampFormat::Milliseconds,
        14..=16 => TimestampFormat::Microseconds,
        17..=19 => TimestampFormat::Nanoseconds,
        _ => {
            warn!(
                "Unknown timestamp format for {} ({}): {} with {} digits",
                exchange, timestamp, timestamp, digit_count
            );
            TimestampFormat::Milliseconds // Assume milliseconds as default
        }
    };
    
    convert_by_format(timestamp, detected_format, exchange)
}

fn convert_by_format(timestamp: i64, format: TimestampFormat, exchange: &str) -> i64 {
    let converted = match format {
        TimestampFormat::Seconds => {
            let result = timestamp * 1000;
            if timestamp < 10_000_000_000 {  // Likely seconds if less than 10 billion
                info!("Converting {} timestamp from seconds to ms: {} -> {}", 
                    exchange, timestamp, result);
            }
            result
        }
        TimestampFormat::Milliseconds => {
            timestamp // Already in milliseconds
        }
        TimestampFormat::Microseconds => {
            let result = timestamp / 1000;
            info!("Converting {} timestamp from microseconds to ms: {} -> {}", 
                exchange, timestamp, result);
            result
        }
        TimestampFormat::Nanoseconds => {
            let result = timestamp / 1_000_000;
            info!("Converting {} timestamp from nanoseconds to ms: {} -> {}", 
                exchange, timestamp, result);
            result
        }
    };
    
    // Sanity check - timestamp should be reasonable (within 10 years of now)
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    
    let ten_years_ms = 10 * 365 * 24 * 60 * 60 * 1000i64;
    
    if (converted - now_ms).abs() > ten_years_ms {
        warn!(
            "Timestamp {} from {} seems unreasonable after conversion to {} (now: {})",
            timestamp, exchange, converted, now_ms
        );
    }
    
    converted
}

/// Detect format based on a sample of timestamps
pub fn detect_format(timestamps: &[i64]) -> TimestampFormat {
    if timestamps.is_empty() {
        return TimestampFormat::Milliseconds;
    }
    
    // Get the most common digit count
    let mut digit_counts = HashMap::new();
    for &ts in timestamps {
        if ts > 0 {
            let digits = ts.to_string().len();
            *digit_counts.entry(digits).or_insert(0) += 1;
        }
    }
    
    let most_common = digit_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(digits, _)| digits)
        .unwrap_or(13);
    
    match most_common {
        1..=10 => TimestampFormat::Seconds,
        11..=13 => TimestampFormat::Milliseconds,
        14..=16 => TimestampFormat::Microseconds,
        17..=19 => TimestampFormat::Nanoseconds,
        _ => TimestampFormat::Milliseconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_normalize_seconds() {
        // 10-digit timestamp (seconds)
        let ts = 1617720181;
        let normalized = normalize_timestamp(ts, "TestExchange");
        assert_eq!(normalized, 1617720181000);
    }
    
    #[test]
    fn test_normalize_milliseconds() {
        // 13-digit timestamp (milliseconds)
        let ts = 1617720181234i64;
        let normalized = normalize_timestamp(ts, "Binance");
        assert_eq!(normalized, ts); // Should remain unchanged
    }
    
    #[test]
    fn test_normalize_microseconds() {
        // 16-digit timestamp (microseconds)
        let ts = 1617720181234567i64;
        let normalized = normalize_timestamp(ts, "Deribit");
        assert_eq!(normalized, 1617720181234i64);
    }
    
    #[test]
    fn test_known_exchanges() {
        // Test known exchange formats
        assert_eq!(normalize_timestamp(1617720181, "Upbit"), 1617720181000);
        assert_eq!(normalize_timestamp(1617720181234i64, "Binance"), 1617720181234i64);
        assert_eq!(normalize_timestamp(1617720181234567i64, "Deribit"), 1617720181234i64);
    }
}