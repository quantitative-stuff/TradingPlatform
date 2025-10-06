use feeder::load_config_ls::LSConfig;
use feeder::secure_keys::SecureKeyLoader;
use reqwest;
use serde_json::{json, Value};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔑 LS Securities Authentication Test");
    println!("====================================");

    // Test 1: Load keys from secure file
    println!("\n1️⃣ Testing secure key loading...");
    let key_file_path = "./docs/api/ls_sec_keys.txt";
    
    let loader = SecureKeyLoader::new(key_file_path.to_string());
    match loader.load_keys() {
        Ok((app_key, secret_key)) => {
            println!("✅ Keys loaded successfully");
            println!("   App Key: {}...{} (length: {})", 
                &app_key[..8.min(app_key.len())], 
                &app_key[app_key.len().saturating_sub(4)..], 
                app_key.len());
            println!("   Secret Key: {}...{} (length: {})", 
                &secret_key[..8.min(secret_key.len())], 
                &secret_key[secret_key.len().saturating_sub(4)..], 
                secret_key.len());
            
            // Test 2: Try authentication with loaded keys
            println!("\n2️⃣ Testing authentication...");
            test_authentication(&app_key, &secret_key).await?;
        }
        Err(e) => {
            println!("❌ Failed to load keys: {}", e);
            return Ok(());
        }
    }

    Ok(())
}

async fn test_authentication(app_key: &str, secret_key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    
    // Prepare the authentication request
    let params = [
        ("grant_type", "client_credentials"),
        ("appkey", app_key),
        ("appsecretkey", secret_key),
        ("scope", "oob"),
    ];

    println!("   📡 Sending authentication request to LS Securities...");
    println!("   URL: https://openapi.ls-sec.co.kr:8080/oauth2/token");
    println!("   Parameters:");
    println!("     grant_type: client_credentials");
    println!("     appkey: {}...{}", &app_key[..8.min(app_key.len())], &app_key[app_key.len().saturating_sub(4)..]);
    println!("     appsecretkey: {}...{}", &secret_key[..8.min(secret_key.len())], &secret_key[secret_key.len().saturating_sub(4)..]);
    println!("     scope: oob");

    let response = client
        .post("https://openapi.ls-sec.co.kr:8080/oauth2/token")
        .form(&params)
        .send()
        .await?;

    let status = response.status();
    let headers = response.headers().clone();
    let body_text = response.text().await?;

    println!("\n   📥 Response received:");
    println!("     Status: {}", status);
    println!("     Headers:");
    for (name, value) in headers.iter() {
        println!("       {}: {:?}", name, value);
    }
    
    if status.is_success() {
        println!("   ✅ Authentication successful!");
        
        // Try to parse the JSON response
        match serde_json::from_str::<Value>(&body_text) {
            Ok(json_response) => {
                println!("   📄 Response body (JSON):");
                println!("{}", serde_json::to_string_pretty(&json_response)?);
                
                if let Some(access_token) = json_response.get("access_token") {
                    if let Some(token_str) = access_token.as_str() {
                        println!("   🎫 Access token received: {}...{}", 
                            &token_str[..16.min(token_str.len())], 
                            &token_str[token_str.len().saturating_sub(8)..]);
                    }
                }
            }
            Err(e) => {
                println!("   ⚠️  Response is not valid JSON: {}", e);
                println!("   📄 Raw response body:");
                println!("{}", body_text);
            }
        }
    } else {
        println!("   ❌ Authentication failed!");
        println!("   📄 Error response body:");
        println!("{}", body_text);
        
        // Try to provide helpful debugging information
        match status.as_u16() {
            400 => println!("   💡 HTTP 400: Bad Request - Check if parameters are correct"),
            401 => println!("   💡 HTTP 401: Unauthorized - Check if app_key and secret_key are valid"),
            403 => println!("   💡 HTTP 403: Forbidden - Check if your account has API access"),
            404 => println!("   💡 HTTP 404: Not Found - Check if the API endpoint URL is correct"),
            500 => println!("   💡 HTTP 500: Internal Server Error - LS Securities server issue"),
            _ => println!("   💡 HTTP {}: See LS Securities API documentation", status.as_u16()),
        }
        
        // Try to parse error response as JSON
        if let Ok(error_json) = serde_json::from_str::<Value>(&body_text) {
            println!("   📄 Error details (JSON):");
            println!("{}", serde_json::to_string_pretty(&error_json)?);
        }
    }

    Ok(())
}