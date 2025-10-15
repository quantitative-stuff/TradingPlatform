/// Checksum validation for orderbook integrity
///
/// Different exchanges use different checksum algorithms:
/// - OKX: CRC32 of top 25 levels
/// - Binance: No checksum (use sequence numbers)
/// - Bybit: MD5 hash of orderbook state

use crc32fast::Hasher;
use md5::{Digest, Md5};
use market_types::{OrderBook, PriceLevel, Exchange};
use anyhow::{Result, bail};
use std::fmt::Write;

/// Checksum type for different exchanges
#[derive(Debug, Clone)]
pub enum ChecksumType {
    None,
    CRC32,
    MD5,
}

/// Calculate checksum for orderbook based on exchange requirements
pub fn calculate_checksum(orderbook: &OrderBook, exchange: Exchange) -> Option<String> {
    match exchange {
        Exchange::OKX => Some(calculate_okx_checksum(orderbook)),
        Exchange::Bybit => Some(calculate_bybit_checksum(orderbook)),
        _ => None, // Binance and others don't use checksums
    }
}

/// Validate orderbook checksum
pub fn validate_checksum(
    orderbook: &OrderBook,
    exchange: Exchange,
    expected_checksum: &str,
) -> Result<()> {
    match exchange {
        Exchange::OKX => {
            let calculated = calculate_okx_checksum(orderbook);
            if calculated != expected_checksum {
                bail!(
                    "OKX checksum mismatch: expected {}, got {}",
                    expected_checksum,
                    calculated
                );
            }
        }
        Exchange::Bybit => {
            let calculated = calculate_bybit_checksum(orderbook);
            if calculated != expected_checksum {
                bail!(
                    "Bybit checksum mismatch: expected {}, got {}",
                    expected_checksum,
                    calculated
                );
            }
        }
        _ => {
            // No checksum validation for this exchange
        }
    }
    Ok(())
}

/// Calculate OKX-style CRC32 checksum
/// Format: "bid1:size1:ask1:size1:bid2:size2:..."
fn calculate_okx_checksum(orderbook: &OrderBook) -> String {
    let mut hasher = Hasher::new();
    let mut checksum_str = String::new();

    // Take top 25 levels from each side
    let max_levels = 25;
    let bid_count = orderbook.bids.len().min(max_levels);
    let ask_count = orderbook.asks.len().min(max_levels);
    let total_levels = bid_count.max(ask_count);

    for i in 0..total_levels {
        // Add bid if available
        if i < bid_count {
            let bid = &orderbook.bids[i];
            write!(
                &mut checksum_str,
                "{}:{}:",
                format_price(bid.price),
                format_quantity(bid.quantity)
            )
            .unwrap();
        }

        // Add ask if available
        if i < ask_count {
            let ask = &orderbook.asks[i];
            write!(
                &mut checksum_str,
                "{}:{}:",
                format_price(ask.price),
                format_quantity(ask.quantity)
            )
            .unwrap();
        }
    }

    // Remove trailing colon
    if checksum_str.ends_with(':') {
        checksum_str.pop();
    }

    hasher.update(checksum_str.as_bytes());
    format!("{:08x}", hasher.finalize())
}

/// Calculate Bybit-style MD5 checksum
fn calculate_bybit_checksum(orderbook: &OrderBook) -> String {
    let mut hasher = Md5::new();
    let mut checksum_str = String::new();

    // Bybit uses all levels in the orderbook
    for bid in &orderbook.bids {
        write!(
            &mut checksum_str,
            "{}:{}|",
            format_price(bid.price),
            format_quantity(bid.quantity)
        )
        .unwrap();
    }

    checksum_str.push_str("||");

    for ask in &orderbook.asks {
        write!(
            &mut checksum_str,
            "{}:{}|",
            format_price(ask.price),
            format_quantity(ask.quantity)
        )
        .unwrap();
    }

    hasher.update(checksum_str.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Format price for checksum (remove unnecessary decimals)
fn format_price(price: f64) -> String {
    if price.fract() == 0.0 {
        format!("{:.0}", price)
    } else {
        let s = format!("{:.8}", price);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Format quantity for checksum
fn format_quantity(quantity: f64) -> String {
    if quantity.fract() == 0.0 {
        format!("{:.0}", quantity)
    } else {
        let s = format!("{:.8}", quantity);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Checksum validator with statistics
pub struct ChecksumValidator {
    exchange: Exchange,
    total_validations: u64,
    failed_validations: u64,
    last_valid_checksum: Option<String>,
    consecutive_failures: u32,
    max_consecutive_failures: u32,
}

impl ChecksumValidator {
    pub fn new(exchange: Exchange) -> Self {
        Self {
            exchange,
            total_validations: 0,
            failed_validations: 0,
            last_valid_checksum: None,
            consecutive_failures: 0,
            max_consecutive_failures: 5, // Trigger resync after 5 failures
        }
    }

    /// Validate orderbook with checksum
    pub fn validate(&mut self, orderbook: &OrderBook, checksum: Option<&str>) -> Result<bool> {
        self.total_validations += 1;

        // If no checksum provided, skip validation
        let checksum = match checksum {
            Some(cs) => cs,
            None => return Ok(true),
        };

        // Calculate and validate
        match validate_checksum(orderbook, self.exchange, checksum) {
            Ok(_) => {
                self.last_valid_checksum = Some(checksum.to_string());
                self.consecutive_failures = 0;
                Ok(true)
            }
            Err(e) => {
                self.failed_validations += 1;
                self.consecutive_failures += 1;

                if self.consecutive_failures >= self.max_consecutive_failures {
                    bail!(
                        "Too many consecutive checksum failures: {}. {}",
                        self.consecutive_failures,
                        e
                    );
                }

                Ok(false)
            }
        }
    }

    /// Get validation statistics
    pub fn stats(&self) -> ChecksumStats {
        ChecksumStats {
            total_validations: self.total_validations,
            failed_validations: self.failed_validations,
            success_rate: if self.total_validations > 0 {
                (self.total_validations - self.failed_validations) as f64
                    / self.total_validations as f64
            } else {
                0.0
            },
            consecutive_failures: self.consecutive_failures,
        }
    }

    /// Reset consecutive failure counter (e.g., after successful resync)
    pub fn reset_failures(&mut self) {
        self.consecutive_failures = 0;
    }
}

#[derive(Debug, Clone)]
pub struct ChecksumStats {
    pub total_validations: u64,
    pub failed_validations: u64,
    pub success_rate: f64,
    pub consecutive_failures: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_okx_checksum() {
        let mut orderbook = OrderBook::new("BTCUSDT".to_string(), Exchange::OKX);

        // Add some test levels
        orderbook.bids.push(PriceLevel {
            price: 100.5,
            quantity: 10.0,
        });
        orderbook.bids.push(PriceLevel {
            price: 100.0,
            quantity: 20.0,
        });

        orderbook.asks.push(PriceLevel {
            price: 101.0,
            quantity: 15.0,
        });
        orderbook.asks.push(PriceLevel {
            price: 101.5,
            quantity: 25.0,
        });

        let checksum = calculate_okx_checksum(&orderbook);
        assert!(!checksum.is_empty());
        assert_eq!(checksum.len(), 8); // CRC32 is 8 hex chars
    }

    #[test]
    fn test_validator() {
        let mut validator = ChecksumValidator::new(Exchange::OKX);
        let mut orderbook = OrderBook::new("BTCUSDT".to_string(), Exchange::OKX);

        orderbook.bids.push(PriceLevel {
            price: 100.0,
            quantity: 10.0,
        });
        orderbook.asks.push(PriceLevel {
            price: 101.0,
            quantity: 10.0,
        });

        // Calculate correct checksum
        let correct_checksum = calculate_okx_checksum(&orderbook);

        // Should pass with correct checksum
        assert!(validator.validate(&orderbook, Some(&correct_checksum)).unwrap());

        // Should fail with wrong checksum
        assert!(!validator.validate(&orderbook, Some("wrongchecksum")).unwrap());

        // Check stats
        let stats = validator.stats();
        assert_eq!(stats.total_validations, 2);
        assert_eq!(stats.failed_validations, 1);
        assert_eq!(stats.success_rate, 0.5);
    }
}