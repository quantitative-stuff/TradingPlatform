use tracing::{warn, debug};
use std::time::{SystemTime, UNIX_EPOCH};

/// Filter to distinguish real timestamps from sequence numbers or other data
/// This is critical for Binance which sends mixed data in the timestamp field

const MIN_VALID_TIMESTAMP: i64 = 1_000_000_000_000; // Jan 2001 in milliseconds
const MAX_VALID_TIMESTAMP: i64 = 2_000_000_000_000; // May 2033 in milliseconds

/// Check if a value is likely a timestamp vs sequence number
pub fn is_valid_timestamp(value: i64, exchange: &str) -> bool {
    // Get current time for validation
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    
    // First check: digit count
    let digit_count = if value <= 0 {
        return false;
    } else {
        value.to_string().len()
    };
    
    // Values with less than 10 digits are definitely not timestamps
    if digit_count < 10 {
        debug!("{}: Value {} with {} digits is a sequence number, not timestamp", 
            exchange, value, digit_count);
        return false;
    }
    
    // Check if it's seconds (10 digits) that need conversion
    if digit_count == 10 {
        // Convert to milliseconds for validation
        let as_millis = value * 1000;
        if as_millis >= MIN_VALID_TIMESTAMP && as_millis <= MAX_VALID_TIMESTAMP {
            // Further check: should be within reasonable range of current time
            let diff_from_now = (as_millis - now_ms).abs();
            let one_year_ms = 365 * 24 * 60 * 60 * 1000i64;
            
            if diff_from_now < one_year_ms * 2 {
                return true; // Valid timestamp in seconds
            }
        }
        return false;
    }
    
    // Check if it's milliseconds (13 digits)
    if digit_count == 13 {
        // Check if within valid range
        if value >= MIN_VALID_TIMESTAMP && value <= MAX_VALID_TIMESTAMP {
            // Further check: should be within reasonable range of current time
            let diff_from_now = (value - now_ms).abs();
            let one_year_ms = 365 * 24 * 60 * 60 * 1000i64;
            
            if diff_from_now < one_year_ms * 2 {
                return true; // Valid timestamp in milliseconds
            } else {
                warn!("{}: Timestamp {} is more than 2 years from now", exchange, value);
            }
        }
    }
    
    // Check microseconds (16 digits)
    if digit_count == 16 {
        let as_millis = value / 1000;
        if as_millis >= MIN_VALID_TIMESTAMP && as_millis <= MAX_VALID_TIMESTAMP {
            return true;
        }
    }
    
    false
}

/// Process a value that might be timestamp or sequence number
/// Returns (is_timestamp, normalized_value)
pub fn process_timestamp_field(value: i64, exchange: &str) -> (bool, i64) {
    if !is_valid_timestamp(value, exchange) {
        // It's a sequence number or invalid data
        return (false, value);
    }
    
    // It's a valid timestamp - normalize it
    let digit_count = value.to_string().len();
    
    let normalized = match digit_count {
        10 => value * 1000,        // Seconds to milliseconds
        13 => value,                // Already milliseconds
        16 => value / 1000,         // Microseconds to milliseconds
        19 => value / 1_000_000,    // Nanoseconds to milliseconds
        _ => value,
    };
    
    (true, normalized)
}

/// Get current timestamp in milliseconds for packets that don't have valid timestamps
pub fn get_current_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Fix Binance-specific issues where sequence numbers are sent as timestamps
pub fn fix_binance_timestamp(value: i64, symbol: &str) -> i64 {
    let (is_timestamp, normalized) = process_timestamp_field(value, "Binance");
    
    if !is_timestamp {
        // It's a sequence number - use current time instead
        let current = get_current_timestamp_ms();
        debug!("Binance {}: Replacing sequence {} with current timestamp {}", 
            symbol, value, current);
        current
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sequence_number_detection() {
        // 9-digit values should be detected as sequence numbers
        assert!(!is_valid_timestamp(134456049, "Test"));
        assert!(!is_valid_timestamp(865110091, "Test"));
        
        // Very small values are sequence numbers
        assert!(!is_valid_timestamp(12345, "Test"));
        assert!(!is_valid_timestamp(999999999, "Test"));
    }
    
    #[test]
    fn test_valid_timestamps() {
        // Valid milliseconds (13 digits, recent)
        assert!(is_valid_timestamp(1757545057527, "Test"));
        
        // Valid seconds (10 digits, recent) 
        assert!(is_valid_timestamp(1757545057, "Test"));
    }
    
    #[test]
    fn test_binance_fix() {
        // Sequence number should be replaced with current time
        let fixed = fix_binance_timestamp(134456049, "TEST-USDT");
        assert!(fixed > 1_000_000_000_000); // Should be current time in ms
        
        // Valid timestamp should be preserved
        let valid = 1757545057527i64;
        let fixed = fix_binance_timestamp(valid, "TEST-USDT");
        assert_eq!(fixed, valid);
    }
}