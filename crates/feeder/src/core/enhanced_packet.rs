use serde::{Deserialize, Serialize};
use crate::core::{TradeData, OrderBookData, get_price_decimals};

/// Enhanced packet format that includes scale factor for precise decoding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedTradePacket {
    // Header
    pub exchange: String,
    pub symbol: String,
    pub timestamp: u64,
    pub price_scale: u8,      // Number of decimals (scale factor = 10^price_scale)
    pub quantity_scale: u8,   // Number of decimals for quantity

    // Scaled data
    pub price_scaled: i64,    // Actual price * 10^price_scale
    pub quantity_scaled: i64, // Actual quantity * 10^quantity_scale
    pub side: String,
    pub trade_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedOrderBookPacket {
    // Header
    pub exchange: String,
    pub symbol: String,
    pub timestamp: u64,
    pub price_scale: u8,      // Number of decimals
    pub quantity_scale: u8,   // Number of decimals for quantity

    // Scaled data
    pub bids: Vec<(i64, i64)>, // [(scaled_price, scaled_quantity)]
    pub asks: Vec<(i64, i64)>, // [(scaled_price, scaled_quantity)]
}

impl EnhancedTradePacket {
    pub fn from_trade_data(trade: &TradeData) -> Self {
        let price_scale = get_price_decimals(&trade.symbol, Some(&trade.exchange));
        let quantity_scale = 8; // Default, should come from precision manager

        // Convert from trade's precision to target precision
        let price_scaled = if trade.price_precision < price_scale {
            trade.price * 10_i64.pow((price_scale - trade.price_precision) as u32)
        } else if trade.price_precision > price_scale {
            trade.price / 10_i64.pow((trade.price_precision - price_scale) as u32)
        } else {
            trade.price
        };

        let quantity_scaled = if trade.quantity_precision < quantity_scale {
            trade.quantity * 10_i64.pow((quantity_scale - trade.quantity_precision) as u32)
        } else if trade.quantity_precision > quantity_scale {
            trade.quantity / 10_i64.pow((trade.quantity_precision - quantity_scale) as u32)
        } else {
            trade.quantity
        };

        Self {
            exchange: trade.exchange.clone(),
            symbol: trade.symbol.clone(),
            timestamp: trade.timestamp,
            price_scale,
            quantity_scale,
            price_scaled,
            quantity_scaled,
            side: "unknown".to_string(), // TradeData doesn't have side field
            trade_id: None, // TradeData doesn't have trade_id field
        }
    }
    
    /// Decode scaled values back to original
    pub fn decode_price(&self) -> f64 {
        self.price_scaled as f64 / 10_f64.powi(self.price_scale as i32)
    }
    
    pub fn decode_quantity(&self) -> f64 {
        self.quantity_scaled as f64 / 10_f64.powi(self.quantity_scale as i32)
    }
    
    /// Create compact binary representation (for efficient UDP)
    pub fn to_binary(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(128);
        
        // Fixed header (1 + 1 + 1 = 3 bytes)
        buffer.push(1u8); // Packet type: 1 = Trade
        buffer.push(self.price_scale);
        buffer.push(self.quantity_scale);
        
        // Exchange name (20 bytes, padded)
        let mut exchange_bytes = self.exchange.as_bytes().to_vec();
        exchange_bytes.resize(20, 0);
        buffer.extend_from_slice(&exchange_bytes);
        
        // Symbol (20 bytes, padded)
        let mut symbol_bytes = self.symbol.as_bytes().to_vec();
        symbol_bytes.resize(20, 0);
        buffer.extend_from_slice(&symbol_bytes);
        
        // Timestamp (8 bytes)
        buffer.extend_from_slice(&self.timestamp.to_be_bytes());
        
        // Price (8 bytes)
        buffer.extend_from_slice(&self.price_scaled.to_be_bytes());
        
        // Quantity (8 bytes)
        buffer.extend_from_slice(&self.quantity_scaled.to_be_bytes());
        
        // Side (1 byte: 0 = sell, 1 = buy)
        buffer.push(if self.side == "buy" { 1 } else { 0 });
        
        // Total: 3 + 20 + 20 + 8 + 8 + 8 + 1 = 68 bytes
        buffer
    }
    
    /// Parse from binary representation
    pub fn from_binary(data: &[u8]) -> Result<Self, String> {
        if data.len() < 68 {
            return Err("Insufficient data".to_string());
        }
        
        if data[0] != 1 {
            return Err("Not a trade packet".to_string());
        }
        
        let price_scale = data[1];
        let quantity_scale = data[2];
        
        let exchange = String::from_utf8_lossy(&data[3..23])
            .trim_end_matches('\0')
            .to_string();
        
        let symbol = String::from_utf8_lossy(&data[23..43])
            .trim_end_matches('\0')
            .to_string();
        
        let timestamp = u64::from_be_bytes([
            data[43], data[44], data[45], data[46],
            data[47], data[48], data[49], data[50],
        ]);
        
        let price_scaled = i64::from_be_bytes([
            data[51], data[52], data[53], data[54],
            data[55], data[56], data[57], data[58],
        ]);
        
        let quantity_scaled = i64::from_be_bytes([
            data[59], data[60], data[61], data[62],
            data[63], data[64], data[65], data[66],
        ]);
        
        let side = if data[67] == 1 { "buy" } else { "sell" }.to_string();
        
        Ok(Self {
            exchange,
            symbol,
            timestamp,
            price_scale,
            quantity_scale,
            price_scaled,
            quantity_scaled,
            side,
            trade_id: None,
        })
    }
}

impl EnhancedOrderBookPacket {
    pub fn from_orderbook_data(orderbook: &OrderBookData) -> Self {
        let price_scale = get_price_decimals(&orderbook.symbol, Some(&orderbook.exchange));
        let quantity_scale = 8; // Default

        let price_precision = orderbook.price_precision;
        let qty_precision = orderbook.quantity_precision;

        // Convert from orderbook's precision to target precision
        let scale_level = |(price, qty): &(i64, i64)| -> (i64, i64) {
            let price_scaled = if price_precision < price_scale {
                price * 10_i64.pow((price_scale - price_precision) as u32)
            } else if price_precision > price_scale {
                price / 10_i64.pow((price_precision - price_scale) as u32)
            } else {
                *price
            };

            let qty_scaled = if qty_precision < quantity_scale {
                qty * 10_i64.pow((quantity_scale - qty_precision) as u32)
            } else if qty_precision > quantity_scale {
                qty / 10_i64.pow((qty_precision - quantity_scale) as u32)
            } else {
                *qty
            };

            (price_scaled, qty_scaled)
        };

        Self {
            exchange: orderbook.exchange.clone(),
            symbol: orderbook.symbol.clone(),
            timestamp: orderbook.timestamp,
            price_scale,
            quantity_scale,
            bids: orderbook.bids.iter().map(scale_level).collect(),
            asks: orderbook.asks.iter().map(scale_level).collect(),
        }
    }
    
    /// Decode a price/quantity pair
    pub fn decode_level(&self, price: i64, quantity: i64) -> (f64, f64) {
        (
            price as f64 / 10_f64.powi(self.price_scale as i32),
            quantity as f64 / 10_f64.powi(self.quantity_scale as i32),
        )
    }
}

/// Enhanced UDP packet format for text transmission (backward compatible)
pub fn create_enhanced_text_packet(trade: &TradeData) -> String {
    let price_scale = get_price_decimals(&trade.symbol, Some(&trade.exchange));
    
    // Include scale in packet: TRADE|scale|exchange|symbol|...
    format!(
        "TRADE|{}|{}",
        price_scale,
        serde_json::to_string(trade).unwrap_or_default()
    )
}

pub fn parse_enhanced_text_packet(packet: &str) -> Result<(u8, String), String> {
    let parts: Vec<&str> = packet.split('|').collect();
    
    if parts.len() < 3 {
        return Err("Invalid packet format".to_string());
    }
    
    match parts[0] {
        "TRADE" => {
            let scale = parts[1].parse::<u8>().unwrap_or(8);
            let json = parts[2..].join("|");
            Ok((scale, json))
        }
        _ => Err("Unknown packet type".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_trade_packet_binary() {
        let trade = TradeData {
            exchange: "Binance".to_string(),
            symbol: "BTC/USDT".to_string(),
            price: 50000.12,
            quantity: 0.5,
            timestamp: 1234567890,
            asset_type: "spot".to_string(),
        };
        
        let packet = EnhancedTradePacket::from_trade_data(&trade);
        let binary = packet.to_binary();
        
        assert_eq!(binary.len(), 68);
        
        let decoded = EnhancedTradePacket::from_binary(&binary).unwrap();
        assert_eq!(decoded.exchange, "Binance");
        assert_eq!(decoded.symbol, "BTC/USDT");
        assert!((decoded.decode_price() - 50000.12).abs() < 0.01);
    }
}