use serde_json::Value;
use anyhow::Result;

/// Information about sequence numbers extracted from exchange messages
#[derive(Debug, Clone, Default)]
pub struct SequenceInfo {
    /// Current update ID
    pub update_id: u64,

    /// First update ID in this message (for snapshot/range messages)
    pub first_update_id: Option<u64>,

    /// Last update ID in this message (for range messages)
    pub last_update_id: Option<u64>,

    /// Previous update ID (for continuity checking)
    pub prev_update_id: Option<u64>,

    /// Whether this is a snapshot message
    pub is_snapshot: bool,
}

/// Trait for extracting sequence information from exchange-specific messages
pub trait SequenceExtractor {
    /// Extract sequence information from a JSON message
    fn extract_sequence(&self, json: &Value) -> Result<SequenceInfo>;

    /// Check if a sequence gap exists
    fn has_gap(&self, last_known: u64, current: &SequenceInfo) -> bool {
        if current.is_snapshot {
            return false; // Snapshots don't have gaps
        }

        // For range updates (like Binance), check if first_update_id follows last_known
        if let Some(first) = current.first_update_id {
            return first != last_known + 1;
        }

        // For single updates, check if update_id follows last_known
        current.update_id != last_known + 1
    }
}

/// Binance-specific sequence extractor
pub struct BinanceSequenceExtractor;

impl SequenceExtractor for BinanceSequenceExtractor {
    fn extract_sequence(&self, json: &Value) -> Result<SequenceInfo> {
        // Handle different Binance message types

        // Depth snapshot from REST
        if json.get("lastUpdateId").is_some() {
            return Ok(SequenceInfo {
                update_id: json["lastUpdateId"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("Missing lastUpdateId"))?,
                first_update_id: None,
                last_update_id: None,
                prev_update_id: None,
                is_snapshot: true,
            });
        }

        // WebSocket depth update
        if json.get("e").and_then(|e| e.as_str()) == Some("depthUpdate") {
            let first = json["U"].as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing U field in depthUpdate"))?;
            let last = json["u"].as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing u field in depthUpdate"))?;

            return Ok(SequenceInfo {
                update_id: last,
                first_update_id: Some(first),
                last_update_id: Some(last),
                prev_update_id: None,
                is_snapshot: false,
            });
        }

        anyhow::bail!("Unknown Binance message format")
    }
}

/// Bybit-specific sequence extractor
pub struct BybitSequenceExtractor;

impl SequenceExtractor for BybitSequenceExtractor {
    fn extract_sequence(&self, json: &Value) -> Result<SequenceInfo> {
        // Handle Bybit orderbook messages
        let data = json.get("data")
            .ok_or_else(|| anyhow::anyhow!("Missing data field in Bybit message"))?;

        // Check message type
        let msg_type = json.get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let is_snapshot = msg_type == "snapshot";

        // Extract update_id (u field)
        let update_id = if data.is_array() {
            // Handle array format
            data[0]["u"].as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing u field in Bybit data"))?
        } else {
            // Handle object format
            data["u"].as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing u field in Bybit data"))?
        };

        // Extract sequence if available
        let prev_seq = if data.is_array() {
            data[0].get("seq").and_then(|s| s.as_u64())
        } else {
            data.get("seq").and_then(|s| s.as_u64())
        };

        Ok(SequenceInfo {
            update_id,
            first_update_id: None,
            last_update_id: None,
            prev_update_id: prev_seq,
            is_snapshot,
        })
    }
}

/// OKX-specific sequence extractor
pub struct OKXSequenceExtractor;

impl SequenceExtractor for OKXSequenceExtractor {
    fn extract_sequence(&self, json: &Value) -> Result<SequenceInfo> {
        // OKX sends data in array format
        let data = json.get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid data field in OKX message"))?;

        // Check action type
        let action = json.get("action")
            .and_then(|a| a.as_str())
            .unwrap_or("");

        let is_snapshot = action == "snapshot";

        // Extract seqId
        let seq_id = data["seqId"].as_u64()
            .or_else(|| data["seqId"].as_str()?.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid seqId in OKX data"))?;

        // Extract prevSeqId if available
        let prev_seq_id = data.get("prevSeqId")
            .and_then(|p| p.as_u64().or_else(|| p.as_str()?.parse().ok()));

        Ok(SequenceInfo {
            update_id: seq_id,
            first_update_id: None,
            last_update_id: None,
            prev_update_id: prev_seq_id,
            is_snapshot,
        })
    }
}

/// Factory function to create appropriate sequence extractor
pub fn create_sequence_extractor(exchange: crate::Exchange) -> Box<dyn SequenceExtractor + Send + Sync> {
    use crate::Exchange;

    match exchange {
        Exchange::Binance | Exchange::BinanceFutures => Box::new(BinanceSequenceExtractor),
        Exchange::Bybit | Exchange::BybitLinear => Box::new(BybitSequenceExtractor),
        Exchange::OKX => Box::new(OKXSequenceExtractor),
        _ => Box::new(DefaultSequenceExtractor),
    }
}

/// Default sequence extractor for exchanges without specific implementation
pub struct DefaultSequenceExtractor;

impl SequenceExtractor for DefaultSequenceExtractor {
    fn extract_sequence(&self, _json: &Value) -> Result<SequenceInfo> {
        // Return default values for exchanges we haven't implemented yet
        Ok(SequenceInfo::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_binance_snapshot_extraction() {
        let extractor = BinanceSequenceExtractor;
        let json = json!({
            "lastUpdateId": 12345,
            "bids": [],
            "asks": []
        });

        let info = extractor.extract_sequence(&json).unwrap();
        assert_eq!(info.update_id, 12345);
        assert!(info.is_snapshot);
    }

    #[test]
    fn test_binance_update_extraction() {
        let extractor = BinanceSequenceExtractor;
        let json = json!({
            "e": "depthUpdate",
            "E": 1234567890,
            "s": "BTCUSDT",
            "U": 100,
            "u": 105,
            "b": [],
            "a": []
        });

        let info = extractor.extract_sequence(&json).unwrap();
        assert_eq!(info.update_id, 105);
        assert_eq!(info.first_update_id, Some(100));
        assert_eq!(info.last_update_id, Some(105));
        assert!(!info.is_snapshot);
    }

    #[test]
    fn test_gap_detection() {
        let extractor = BinanceSequenceExtractor;

        // No gap case
        let current = SequenceInfo {
            update_id: 101,
            first_update_id: Some(101),
            last_update_id: Some(105),
            prev_update_id: None,
            is_snapshot: false,
        };
        assert!(!extractor.has_gap(100, &current));

        // Gap case
        let current_with_gap = SequenceInfo {
            update_id: 110,
            first_update_id: Some(108),
            last_update_id: Some(110),
            prev_update_id: None,
            is_snapshot: false,
        };
        assert!(extractor.has_gap(105, &current_with_gap));
    }
}