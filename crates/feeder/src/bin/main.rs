use tokio;
use reqwest;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures::{SinkExt, StreamExt};
use std::fs::File;
use std::io::Read;
use chrono::Local;
use futures::future::join_all;
use std::path::Path;
use anyhow::{Result, anyhow};

const APP_KEY: &str = "PS5lH0u8jPeQCNoIKWGCORiK5BwrNFrK5osl";
const SECRET_KEY: &str = "OefKITXw3Ic7zcsOHKkR3LadRHbstUY8";
const BASE_URL: &str = "https://openapi.ls-sec.co.kr:8080";
const WS_BASE_URL: &str = "wss://openapi.ls-sec.co.kr:9443/websocket";

#[derive(Debug, Serialize, Deserialize)]
struct TokenResponse {
    access_token: String,
    // Add other fields if needed
}

#[derive(Debug, Serialize, Deserialize)]
struct WebSocketMessage {
    header: Header,
    body: Body,
}

#[derive(Debug, Serialize, Deserialize)]
struct Header {
    token: String,
    tr_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Body {
    tr_cd: String,
    tr_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Ticker {
    ticker: String,
}

async fn get_access_token() -> Result<String> {
    let client = reqwest::Client::new();
    
    let params = [
        ("grant_type", "client_credentials"),
        ("appkey", APP_KEY),
        ("appsecretkey", SECRET_KEY),
        ("scope", "oob"),
    ];

    let response = client
        .post(&format!("{}/oauth2/token", BASE_URL))
        .query(&params)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await?;

    // Print the status code
    println!("Status: {}", response.status());
    
    // Get and print the raw response text
    let text = response.text().await?;
    // println!("한글깨짐");
    // println!("Raw response: {}", text);

    // Try to parse the response as JSON and print any error
    match serde_json::from_str::<TokenResponse>(&text) {
        Ok(token_response) => {
            // println!("Successfully parsed token: {}", token_response.access_token);
            Ok(token_response.access_token)
        },
        Err(e) => {
            Err(anyhow!("Failed to parse token response: {}. Raw response: {}", e, text))
        }
    }
}

fn read_tickers_from_json(file_path: &str) -> Result<Vec<String>> {
    let mut file = File::open(file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    
    let tickers: Vec<Ticker> = serde_json::from_str(&contents)?;
    println!("check tickers from json {:?}", tickers);

    Ok(tickers.into_iter().map(|t| t.ticker).collect())
}

async fn connect_and_save(ticker: String, tr_cd: String, access_token: String) -> Result<()> {
    let current_date = Local::now().format("%Y-%m-%d").to_string();
    let file_name = format!("{}.{}", ticker, tr_cd);
    let file_path = format!("data/api_data/{}/{}", current_date, file_name);

    // Ensure directory exists
    if let Some(parent) = Path::new(&file_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (ws_stream, _) = connect_async(WS_BASE_URL).await?;
    let (mut write, mut read) = ws_stream.split();

    let message = WebSocketMessage {
        header: Header {
            token: access_token,
            tr_type: "3".to_string(),
        },
        body: Body {
            tr_cd,
            tr_key: ticker,
        },
    };

    let json_message = serde_json::to_string(&message)?;
    write.send(Message::text(json_message)).await?;
    
    let mut file = tokio::fs::File::create(file_path).await?;

    while let Some(msg) = read.next().await {
        let msg = msg?;
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string();
        let data = format!("{}: {}\n", timestamp, msg);
        
        tokio::io::AsyncWriteExt::write_all(&mut file, data.as_bytes()).await?;        
        // Add your exit condition here if needed
        // if msg.contains("exit_condition") {
        //     break;
        // }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("start to get token");
    let access_token = get_access_token().await?;
    
    // let file_path = "./k100_stock.json";
    // let tickers = read_tickers_from_json(file_path)?;
    let tickers = vec![
        "005930".to_string(),  // Samsung Electronics
        "000660".to_string(),  // SK Hynix
        "035420".to_string(),  // NAVER
    ];
    let tr_cd_list = vec!["JH0".to_string()];
    
    let mut tasks = Vec::new();
    
    for ticker in tickers {
        for tr_cd in &tr_cd_list {
            let task = connect_and_save(
                ticker.clone(),
                tr_cd.clone(),
                access_token.clone(),
            );
            tasks.push(task);
        }
    }

    join_all(tasks).await;
    Ok(())
}
