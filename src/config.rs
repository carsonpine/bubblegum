use solana_sdk::pubkey::Pubkey;
use std::env;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub helius_rpc_url: String,
    pub postgres_url: String,
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub program_id: Pubkey,
    pub start_slot: Option<u64>,
    pub end_slot: Option<u64>,
    pub batch_size: usize,
    pub idl_path: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        fn non_empty(key: &'static str) -> Result<String, ConfigError> {
            let val = env::var(key).map_err(|_| ConfigError::MissingEnv(key))?;
            if val.is_empty() {
                Err(ConfigError::MissingEnv(key))
            } else {
                Ok(val)
            }
        }

        fn optional(key: &str) -> Option<String> {
            env::var(key).ok().filter(|s| !s.is_empty())
        }

        let helius_rpc_url = non_empty("HELIUS_RPC_URL")?;
        if !helius_rpc_url.starts_with("https://") {
            return Err(ConfigError::InvalidRpcUrl);
        }

        let postgres_url = non_empty("POSTGRES_URL")?;

        let clickhouse_url = non_empty("CLICKHOUSE_URL")?;
        let clickhouse_user = optional("CLICKHOUSE_USER").unwrap_or_else(|| "default".to_string());
        let clickhouse_password = optional("CLICKHOUSE_PASSWORD").unwrap_or_default();

        let program_id_str = non_empty("PROGRAM_ID")?;
        let program_id =
            Pubkey::from_str(&program_id_str).map_err(|_| ConfigError::InvalidProgramId)?;

        let start_slot = optional("START_SLOT").and_then(|s| s.parse().ok());

        let end_slot = optional("END_SLOT").and_then(|s| s.parse().ok());

        let batch_size = optional("BATCH_SIZE")
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let idl_path = optional("IDL_PATH");

        Ok(Config {
            helius_rpc_url,
            postgres_url,
            clickhouse_url,
            clickhouse_user,
            clickhouse_password,
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
