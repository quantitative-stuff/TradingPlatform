use crate::error::Result;
use std::path::PathBuf;

use std::fs::OpenOptions;
use std::io::Write;

#[derive(Clone)]
pub struct FileStorage;

pub struct SpreadData {
    pub timestamp: i64,
    pub symbol: String,
    pub exchange1: String,
    pub exchange2: String,
    pub price1: f64,
    pub price2: f64,
    pub spread: f64,
    pub spread_percent: f64,
}

impl FileStorage {
    pub fn new() -> Self {
        FileStorage
    }

    pub fn store_spread(spread: &SpreadData) -> Result<()> {
        // Define path directly in this function
        let file_path = PathBuf::from("data")
            .join("spreads")
            .join(format!("spreads_{}.csv", spread.symbol));
        
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)?;

        writeln!(
            file,
            "{},{},{},{},{},{},{:.2}", 
            spread.timestamp,
            spread.exchange1,
            spread.exchange2,
            spread.price1,
            spread.price2,
            spread.spread,
            spread.spread_percent
        )?;

        println!("Successfully stored spread data");  // Debug print

        Ok(())
    }

}

// #[async_trait]
// impl Storage for FileStorage {
//     async fn store(&self, data: &MarketData) -> Result<()> {
//         let date = Local::now().format("%Y-%m-%d");
//         let filename = format!("{}_{}_{}_{}.json", 
//             date, data.exchange, data.symbol, 
//             match data.data_type {
//                 MarketDataType::Trade(_) => "trade",
//                 MarketDataType::OrderBook(_) => "orderbook",
//             });
//         let path = self.base_path.join(date.to_string()).join(filename);

//         if let Some(parent) = path.parent() {
//             tokio::fs::create_dir_all(parent).await?;
//         }

//         let mut file = tokio::fs::OpenOptions::new()
//             .create(true)
//             .append(true)
//             .open(path)
//             .await?;

//         let line = serde_json::to_string(&data)?;
//         tokio::io::AsyncWriteExt::write_all(&mut file, format!("{}\n", line).as_bytes()).await?;
        
//         Ok(())
//     }
// }