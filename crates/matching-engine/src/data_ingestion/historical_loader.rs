use crate::types::{OrderBookUpdate, Trade, Exchange, Side, PriceLevel, Timestamp};
use anyhow::{Result, Context};
use polars::prelude::*;
use std::path::Path;
use tokio::sync::mpsc;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::Read;
use std::collections::HashMap;

pub struct HistoricalDataLoader {
    base_path: String,
    orderbook_tx: mpsc::Sender<OrderBookUpdate>,
    trades_tx: mpsc::Sender<Trade>,
}

impl HistoricalDataLoader {
    pub fn new(
        base_path: String,
        buffer_size: usize
    ) -> (Self, mpsc::Receiver<OrderBookUpdate>, mpsc::Receiver<Trade>) {
        let (orderbook_tx, orderbook_rx) = mpsc::channel(buffer_size);
        let (trades_tx, trades_rx) = mpsc::channel(buffer_size);

        (
            Self {
                base_path,
                orderbook_tx,
                trades_tx,
            },
            orderbook_rx,
            trades_rx,
        )
    }

    pub async fn load_orderbook_data(
        &self,
        symbol: &str,
        date: &str,
    ) -> Result<()> {
        // Path format: D:/data/binance_futures/incremental_book_L2/BTCUSDT/2025-07-01_BTCUSDT.csv.gz
        let file_path = format!(
            "{}/incremental_book_L2/{}/{}_{}.csv.gz",
            self.base_path, symbol, date, symbol
        );

        println!("Loading orderbook data from: {}", file_path);

        // Read and decompress the gzipped CSV
        let file = File::open(&file_path)
            .with_context(|| format!("Failed to open file: {}", file_path))?;
        let mut decoder = GzDecoder::new(file);
        let mut csv_content = String::new();
        decoder.read_to_string(&mut csv_content)?;

        // Parse CSV with Polars
        let df = CsvReader::new(std::io::Cursor::new(csv_content))
            .has_header(true)
            .finish()?;

        // Group updates by timestamp to reconstruct full orderbook snapshots
        let mut current_timestamp = 0i64;
        let mut bid_updates: Vec<PriceLevel> = Vec::new();
        let mut ask_updates: Vec<PriceLevel> = Vec::new();
        let mut is_snapshot = false;

        for idx in 0..df.height() {
            let timestamp = df.column("timestamp")?
                .i64()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing timestamp"))?;

            let side = df.column("side")?
                .str()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing side"))?;

            let price = df.column("price")?
                .f64()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing price"))?;

            let amount = df.column("amount")?
                .f64()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing amount"))?;

            let is_snap = df.column("is_snapshot")?
                .bool()?
                .get(idx)
                .unwrap_or(false);

            // If we have a new timestamp or snapshot, send the accumulated update
            if timestamp != current_timestamp || is_snap {
                if !bid_updates.is_empty() || !ask_updates.is_empty() {
                    let update = OrderBookUpdate {
                        exchange: Exchange::Binance,
                        symbol: symbol.to_string(),
                        timestamp: current_timestamp,
                        bids: bid_updates.clone(),
                        asks: ask_updates.clone(),
                        is_snapshot,
                    };
                    self.orderbook_tx.send(update).await?;
                }

                // Reset for new timestamp
                current_timestamp = timestamp;
                bid_updates.clear();
                ask_updates.clear();
                is_snapshot = is_snap;
            }

            // Add to appropriate side
            let level = PriceLevel { price, quantity: amount };
            match side {
                "bid" => bid_updates.push(level),
                "ask" => ask_updates.push(level),
                _ => {}
            }
        }

        // Send final update
        if !bid_updates.is_empty() || !ask_updates.is_empty() {
            let update = OrderBookUpdate {
                exchange: Exchange::Binance,
                symbol: symbol.to_string(),
                timestamp: current_timestamp,
                bids: bid_updates,
                asks: ask_updates,
                is_snapshot,
            };
            self.orderbook_tx.send(update).await?;
        }

        println!("Loaded {} orderbook rows", df.height());
        Ok(())
    }

    pub async fn load_trade_data(
        &self,
        symbol: &str,
        date: &str,
    ) -> Result<()> {
        // Path format: D:/data/binance_futures/trades/BTCUSDT/2025-07-01_BTCUSDT.csv.gz
        let file_path = format!(
            "{}/trades/{}/{}_{}.csv.gz",
            self.base_path, symbol, date, symbol
        );

        println!("Loading trade data from: {}", file_path);

        // Read and decompress the gzipped CSV
        let file = File::open(&file_path)
            .with_context(|| format!("Failed to open file: {}", file_path))?;
        let mut decoder = GzDecoder::new(file);
        let mut csv_content = String::new();
        decoder.read_to_string(&mut csv_content)?;

        // Parse CSV with Polars
        let df = CsvReader::new(std::io::Cursor::new(csv_content))
            .has_header(true)
            .finish()?;

        // Process each trade
        for idx in 0..df.height() {
            let timestamp = df.column("timestamp")?
                .i64()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing timestamp"))?;

            let price = df.column("price")?
                .f64()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing price"))?;

            let amount = df.column("amount")?
                .f64()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing amount"))?;

            let side_str = df.column("side")?
                .str()?
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Missing side"))?;

            let trade_id = df.column("id")?
                .str()?
                .get(idx)
                .unwrap_or("unknown")
                .to_string();

            let side = match side_str {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                _ => Side::Sell,
            };

            let trade = Trade {
                exchange: Exchange::Binance,
                symbol: symbol.to_string(),
                timestamp,
                price,
                quantity: amount,
                side,
                trade_id,
            };

            self.trades_tx.send(trade).await?;
        }

        println!("Loaded {} trades", df.height());
        Ok(())
    }

}