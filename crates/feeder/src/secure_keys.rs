use std::fs;
use crate::error::{Result, Error};
use tracing::{info, warn};
use base64::{Engine as _, engine::general_purpose};

/// Utility for loading encrypted or plain text API keys securely
pub struct SecureKeyLoader {
    key_file_path: String,
    encryption_key: Option<String>,
}

impl SecureKeyLoader {
    pub fn new(key_file_path: String) -> Self {
        Self {
            key_file_path,
            encryption_key: std::env::var("LS_ENCRYPTION_KEY").ok(),
        }
    }

    /// Load API keys from file, with optional decryption
    pub fn load_keys(&self) -> Result<(String, String)> {
        let content = fs::read_to_string(&self.key_file_path)
            .map_err(|e| Error::Config(format!("Failed to read key file {}: {}", self.key_file_path, e)))?;

        let mut app_key = String::new();
        let mut secret_key = String::new();

        // Parse the key file (expecting format like: appkey = 'value')
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = self.parse_key_line(line) {
                let decrypted_value = if self.encryption_key.is_some() {
                    self.decrypt_value(&value)?
                } else {
                    value
                };

                match key.as_str() {
                    "appkey" => app_key = decrypted_value,
                    "secretkey" => secret_key = decrypted_value,
                    _ => warn!("Unknown key in config file: {}", key),
                }
            }
        }

        if app_key.is_empty() {
            return Err(Error::Config("Missing appkey in key file".into()));
        }
        if secret_key.is_empty() {
            return Err(Error::Config("Missing secretkey in key file".into()));
        }

        info!("Successfully loaded LS Securities API keys from {}", self.key_file_path);
        Ok((app_key, secret_key))
    }

    fn parse_key_line(&self, line: &str) -> Option<(String, String)> {
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let value_part = line[eq_pos + 1..].trim();
            
            // Remove quotes if present
            let value = if (value_part.starts_with('\'') && value_part.ends_with('\'')) ||
                          (value_part.starts_with('"') && value_part.ends_with('"')) {
                value_part[1..value_part.len()-1].to_string()
            } else {
                value_part.to_string()
            };
            
            Some((key, value))
        } else {
            None
        }
    }

    fn decrypt_value(&self, encrypted_value: &str) -> Result<String> {
        // Simple XOR encryption for demonstration
        // In production, use proper encryption like AES
        if let Some(encryption_key) = &self.encryption_key {
            let key_bytes = encryption_key.as_bytes();
            let encrypted_bytes = general_purpose::STANDARD.decode(encrypted_value)
                .map_err(|e| Error::Config(format!("Failed to decode encrypted value: {}", e)))?;
            
            let mut decrypted = Vec::new();
            for (i, &byte) in encrypted_bytes.iter().enumerate() {
                let key_byte = key_bytes[i % key_bytes.len()];
                decrypted.push(byte ^ key_byte);
            }
            
            String::from_utf8(decrypted)
                .map_err(|e| Error::Config(format!("Failed to decrypt value: {}", e)))
        } else {
            Ok(encrypted_value.to_string())
        }
    }

    /// Encrypt a value using the encryption key (for storing encrypted keys)
    #[allow(dead_code)]
    pub fn encrypt_value(&self, plain_value: &str) -> Result<String> {
        if let Some(encryption_key) = &self.encryption_key {
            let key_bytes = encryption_key.as_bytes();
            let plain_bytes = plain_value.as_bytes();
            
            let mut encrypted = Vec::new();
            for (i, &byte) in plain_bytes.iter().enumerate() {
                let key_byte = key_bytes[i % key_bytes.len()];
                encrypted.push(byte ^ key_byte);
            }
            
            Ok(general_purpose::STANDARD.encode(encrypted))
        } else {
            Err(Error::Config("No encryption key available".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_parse_key_line() {
        let loader = SecureKeyLoader::new("test".to_string());
        
        assert_eq!(
            loader.parse_key_line("appkey = 'test123'"),
            Some(("appkey".to_string(), "test123".to_string()))
        );
        
        assert_eq!(
            loader.parse_key_line("secretkey=\"secret456\""),
            Some(("secretkey".to_string(), "secret456".to_string()))
        );
        
        assert_eq!(
            loader.parse_key_line("key=value"),
            Some(("key".to_string(), "value".to_string()))
        );
    }

    #[test]
    fn test_load_keys_from_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_keys.txt");
        
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "appkey = 'PSYV6tFryhLcNVwQeQb7yY7DhpA5a2QXQ9ad'").unwrap();
        writeln!(file, "secretkey='58qiCMEr1uwAEMz0y6grxWDabb80kSch'").unwrap();
        
        let loader = SecureKeyLoader::new(file_path.to_string_lossy().to_string());
        let (app_key, secret_key) = loader.load_keys().unwrap();
        
        assert_eq!(app_key, "PSYV6tFryhLcNVwQeQb7yY7DhpA5a2QXQ9ad");
        assert_eq!(secret_key, "58qiCMEr1uwAEMz0y6grxWDabb80kSch");
    }
}