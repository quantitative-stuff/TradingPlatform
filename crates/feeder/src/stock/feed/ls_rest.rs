use reqwest::{header::{HeaderMap, HeaderValue}, Client};
use serde_json::{json, Value};
use anyhow::Result;
use crate::load_config_ls::LSConfig;
use std::fs::{self};
use std::path::Path;
use serde::Deserialize;
use tracing::{debug, info, error};

pub struct LSClient {
    client: Client,
    config: LSConfig,
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: String,
}

impl LSClient {

    pub fn new(config: LSConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn create_headers(&self, access_token: &str, tr_cd: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json; charset=utf-8"));
        headers.insert("authorization", HeaderValue::from_str(&format!("Bearer {}", access_token))?);
        headers.insert("tr_cd", HeaderValue::from_str(tr_cd)?);
        headers.insert("tr_cont", HeaderValue::from_static("N"));
        headers.insert("tr_cont_key", HeaderValue::from_static(""));
        Ok(headers)
    }

    pub async fn get_access_token(&self) -> Result<String> {

        // Create form parameters
        let params = [
            ("grant_type", "client_credentials"),
            ("appkey", &self.config.app_key),
            ("appsecretkey", &self.config.secret_key),
            ("scope", "oob"),
        ];

        info!("Sending access token request");
        let response = self.client
            .post("https://openapi.ls-sec.co.kr:8080/oauth2/token")
            .header("content-type", "application/x-www-form-urlencoded")
            .form(&params)  // Use .form() instead of .json() for x-www-form-urlencoded
            .send()
            .await?;
        
        let response_text = response.text().await?;
        debug!("Raw response: {}", response_text);

        // Parse the response text back to JSON
        let token_response: AccessTokenResponse = serde_json::from_str(&response_text)
            .map_err(|e| {
                error!("Failed to parse response: {:?}", e);
                error!("Response content: {}", response_text);
                e
            })?;
        Ok(token_response.access_token)
    }

    pub async fn get_sector_info(&self, access_token: &str) -> Result<Value> {
        let headers = self.create_headers(access_token, "t8424")?;
        let response = self.client
            .post("https://openapi.ls-sec.co.kr:8080/indtp/market-data")
            .headers(headers)
            .json(&json!({
                "t8424InBlock": {
                    "gubun1": ""  // K200 sectors
                }
            }))
            .send()
            .await?;

        let data: Value = response.json().await?;
        Ok(data)
    }

    pub async fn get_stock_info(&self, access_token: &str, upcode: i32) -> Result<Value> {
        let headers = self.create_headers(access_token, "t1516")?;
        let response = self.client
            .post("https://openapi.ls-sec.co.kr:8080/indtp/market-data")
            .headers(headers)
            .json(&json!({
                "t1516InBlock": {
                    "upcode": upcode.to_string(),
                    "gubun": "1",
                    "shcode": ""
                }
            }))
            .send()
            .await?;

        let data: Value = response.json().await?;
        Ok(data)
    }
}


pub mod utils {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    pub fn save_to_json(data: &Value, filename: &str) -> Result<()> {
        // Create directory path
        let dir_path = Path::new("./data/json");
        
        // Create directories if they don't exist
        fs::create_dir_all(dir_path)?;
        
        // Create full file path
        let file_path = dir_path.join(filename);
        
        // Create and write to file
        let mut file = File::create(file_path)?;
        let json_string = serde_json::to_string_pretty(data)?;
        file.write_all(json_string.as_bytes())?;
        
        Ok(())
    }
}
