use anyhow::Result;
use std::fs::File;
use std::sync::Arc;
use serde_json::Value;
use tokio::task::JoinHandle;
use std::time::Duration;
use tracing::{info, warn, error};

use crate::core::{Feeder, SymbolMapper};
use crate::crypto::feed::{BinanceExchange, BybitExchange, UpbitExchange, CoinbaseExchange, OkxExchange, DeribitExchange, BithumbExchange};
use crate::load_config::ExchangeConfig;

#[derive(Clone)]
pub struct ExtendedExchangeConfig {
    pub base_config: ExchangeConfig,
    pub spot_symbols: Vec<String>,
    pub futures_symbols: Vec<String>,
    pub krw_symbols: Vec<String>,
}

impl ExtendedExchangeConfig {
    pub fn load_full_config(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let json: Value = serde_json::from_reader(file)?;

        let spot_symbols = json["subscribe_data"]["spot_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let futures_symbols = json["subscribe_data"]["futures_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let krw_symbols = json["subscribe_data"]["krw_symbols"]
            .as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
            .unwrap_or_default();

        let base_config = ExchangeConfig::new(path)?;

        Ok(Self {
            base_config,
            spot_symbols,
            futures_symbols,
            krw_symbols,
        })
    }
}

pub async fn spawn_binance_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_binance = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "futures" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_binance.subscribe_data.codes = all_symbols;
        let mut exchange = BinanceExchange::new(config_for_binance, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Binance: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Binance: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Binance: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Binance start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Binance subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Binance connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

pub async fn spawn_bybit_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_bybit = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "linear" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_bybit.subscribe_data.codes = all_symbols;
        let mut exchange = BybitExchange::new(config_for_bybit, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Bybit: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Bybit: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Bybit: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Bybit start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Bybit subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Bybit connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

pub async fn spawn_upbit_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut complete_config = config.base_config.clone();
        let mut all_symbols = complete_config.subscribe_data.codes.clone();
        all_symbols.extend(config.krw_symbols.clone());
        complete_config.subscribe_data.codes = all_symbols;

        let mut exchange = UpbitExchange::new(complete_config, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Upbit: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Upbit: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Upbit: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Upbit start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Upbit subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Upbit connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

pub async fn spawn_coinbase_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_coinbase = config.base_config.clone();

        // Coinbase only supports spot trading, so we only use spot_symbols
        // And Coinbase reads from spot_symbols field, not codes field
        config_for_coinbase.subscribe_data.spot_symbols = config.spot_symbols.clone();

        let mut exchange = CoinbaseExchange::new(config_for_coinbase, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Coinbase: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Coinbase: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Coinbase: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Coinbase start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Coinbase subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Coinbase connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

pub async fn spawn_okx_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_okx = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.spot_symbols.clone()),
                "swap" | "futures" => all_symbols.extend(config.futures_symbols.clone()),
                _ => {},
            }
        }

        config_for_okx.subscribe_data.codes = all_symbols;
        let mut exchange = OkxExchange::new(config_for_okx, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("OKX: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("OKX: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("OKX: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("OKX start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("OKX subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("OKX connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

pub async fn spawn_deribit_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_deribit = config.base_config.clone();

        // Deribit reads from futures_symbols and option_symbols fields directly
        // It doesn't use the codes field, so we need to set these specific fields
        config_for_deribit.subscribe_data.futures_symbols = config.futures_symbols.clone();
        // Note: option_symbols would come from config if we had them, for now using empty
        config_for_deribit.subscribe_data.option_symbols = Vec::new();

        let mut exchange = DeribitExchange::new(config_for_deribit, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Deribit: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Deribit: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Deribit: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Deribit start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Deribit subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Deribit connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

pub async fn spawn_bithumb_exchange(
    config: ExtendedExchangeConfig,
    symbol_mapper: Arc<SymbolMapper>
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut config_for_bithumb = config.base_config.clone();
        let mut all_symbols = Vec::new();

        for asset_type in &config.base_config.feed_config.asset_type {
            match asset_type.as_str() {
                "spot" => all_symbols.extend(config.krw_symbols.clone()),
                _ => {},
            }
        }

        config_for_bithumb.subscribe_data.codes = all_symbols;
        let mut exchange = BithumbExchange::new(config_for_bithumb, symbol_mapper);

        loop {
            match exchange.connect().await {
                Ok(_) => {
                    info!("Bithumb: Connected");
                    match exchange.subscribe().await {
                        Ok(_) => {
                            info!("Bithumb: Subscribed");
                            match exchange.start().await {
                                Ok(_) => {
                                    warn!("Bithumb: Connection lost, reconnecting...");
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    error!("Bithumb start error: {}", e);
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Bithumb subscribe error: {}", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Bithumb connect error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}