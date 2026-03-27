use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub helius_rpc_url: String,
    pub postgres_url: String,
    pub clickhouse_url: String,
    pub program_id: Pubkey,
    pub start_slot: Option<u64>,
    pub end_slot: Option<u64>,
    pub batch_size: usize,
    pub idl_path: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let helius_rpc_url =
            env::var("HELIUS_RPC_URL").map_err(|_| ConfigError::MissingEnv("HELIUS_RPC_URL"))?;
        if !helius_rpc_url.starts_with("https://") {
            return Err(ConfigError::InvalidRpcUrl);
        }

        let postgres_url =
            env::var("POSTGRES_URL").map_err(|_| ConfigError::MissingEnv("POSTGRES_URL"))?;

        let clickhouse_url =
            env::var("CLICKHOUSE_URL").map_err(|_| ConfigError::MissingEnv("CLICKHOUSE_URL"))?;

        let program_id_str =
            env::var("PROGRAM_ID").map_err(|_| ConfigError::MissingEnv("PROGRAM_ID"))?;
        let program_id =
            Pubkey::from_str(&program_id_str).map_err(|_| ConfigError::InvalidProgramId)?;

        let start_slot = env::var("START_SLOT").ok().and_then(|s| s.parse().ok());

        let end_slot = env::var("END_SLOT").ok().and_then(|s| s.parse().ok());

        let batch_size = env::var("BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let idl_path = env::var("IDL_PATH").ok();

        Ok(Config {
            helius_rpc_url,
            postgres_url,
            clickhouse_url,
            program_id,
            start_slot,
            end_slot,
            batch_size,
            idl_path,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing environment variable: {0}")]
    MissingEnv(&'static str),
    #[error("invalid RPC URL")]
    InvalidRpcUrl,
    #[error("invalid program ID")]
    InvalidProgramId,
}
