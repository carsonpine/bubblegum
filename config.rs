use anyhow::{anyhow, Result};
use dotenvy::dotenv;
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
pub struct RawConfig {
    pub helius_rpc_url: String,
    pub postgres_url: String,
    pub clickhouse_url: String,
    pub clickhouse_user: Option<String>,
    pub clickhouse_password: Option<String>,
    pub clickhouse_db: Option<String>,
    pub program_id: String,
    pub start_slot: Option<u64>,
    pub end_slot: Option<u64>,
    pub batch_size: Option<usize>,
    pub idl_path: Option<String>,
    pub api_port: Option<u16>,
    pub log_level: Option<String>,
    pub rpc_rate_limit: Option<u32>,
    pub db_max_connections: Option<u32>,
    pub ch_batch_size: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub helius_rpc_url: String,
    pub postgres_url: String,
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub clickhouse_db: String,
    pub program_id: Pubkey,
    pub program_id_str: String,
    pub start_slot: Option<u64>,
    pub end_slot: Option<u64>,
    pub batch_size: usize,
    pub idl_path: Option<String>,
    pub api_port: u16,
    pub log_level: String,
    pub rpc_rate_limit: u32,
    pub db_max_connections: u32,
    pub ch_batch_size: usize,
}

impl Config {
    pub fn load() -> Result<Self> {
        dotenv().ok();

        let raw = envy::from_env::<RawConfig>()
            .map_err(|e| anyhow!("Failed to read environment variables: {}", e))?;

        Self::validate_and_build(raw)
    }

    fn validate_and_build(raw: RawConfig) -> Result<Self> {
        if !raw.helius_rpc_url.starts_with("http://")
            && !raw.helius_rpc_url.starts_with("https://")
        {
            return Err(anyhow!(
                "HELIUS_RPC_URL must start with http:// or https://, got: {}",
                raw.helius_rpc_url
            ));
        }

        if raw.helius_rpc_url.trim().is_empty() {
            return Err(anyhow!("HELIUS_RPC_URL cannot be empty"));
        }

        if !raw.postgres_url.starts_with("postgres://")
            && !raw.postgres_url.starts_with("postgresql://")
        {
            return Err(anyhow!(
                "POSTGRES_URL must start with postgres:// or postgresql://, got: {}",
                raw.postgres_url
            ));
        }

        let program_id = Pubkey::from_str(&raw.program_id).map_err(|e| {
            anyhow!(
                "PROGRAM_ID '{}' is not a valid Solana pubkey: {}",
                raw.program_id,
                e
            )
        })?;

        if let (Some(start), Some(end)) = (raw.start_slot, raw.end_slot) {
            if start >= end {
                return Err(anyhow!(
                    "START_SLOT ({}) must be less than END_SLOT ({})",
                    start,
                    end
                ));
            }
        }

        let batch_size = raw.batch_size.unwrap_or(100);
        if batch_size == 0 || batch_size > 1000 {
            return Err(anyhow!(
                "BATCH_SIZE must be between 1 and 1000, got: {}",
                batch_size
            ));
        }

        let clickhouse_url = if raw.clickhouse_url.trim().is_empty() {
            return Err(anyhow!("CLICKHOUSE_URL cannot be empty"));
        } else {
            raw.clickhouse_url.clone()
        };

        Ok(Config {
            helius_rpc_url: raw.helius_rpc_url,
            postgres_url: raw.postgres_url,
            clickhouse_url,
            clickhouse_user: raw.clickhouse_user.unwrap_or_else(|| "indexer".to_string()),
            clickhouse_password: raw
                .clickhouse_password
                .unwrap_or_else(|| "changeme".to_string()),
            clickhouse_db: raw
                .clickhouse_db
                .unwrap_or_else(|| "solana_indexer".to_string()),
            program_id,
            program_id_str: raw.program_id,
            start_slot: raw.start_slot,
            end_slot: raw.end_slot,
            batch_size,
            idl_path: raw.idl_path,
            api_port: raw.api_port.unwrap_or(3000),
            log_level: raw
                .log_level
                .unwrap_or_else(|| "info".to_string())
                .to_lowercase(),
            rpc_rate_limit: raw.rpc_rate_limit.unwrap_or(10),
            db_max_connections: raw.db_max_connections.unwrap_or(10),
            ch_batch_size: raw.ch_batch_size.unwrap_or(1000),
        })
    }
}