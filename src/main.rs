use anyhow::{Context, Result};
use std::sync::Arc;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod config;
mod db;
mod decoder;
mod idl;
mod indexer;
mod rpc;

use api::{serve, AppState};
use config::Config;
use db::{ClickhouseDb, PostgresDb};
use idl::load_idl;
use indexer::{wait_for_shutdown, Indexer};
use rpc::{HeliusRpcClient, RpcClientConfig};
use solana_sdk::commitment_config::CommitmentConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load().context("Failed to load configuration")?;

    init_logging(&config.log_level);

    tracing::info!("Bubblegum Indexer starting...");
    tracing::info!("Program ID: {}", config.program_id_str);
    tracing::info!("API port: {}", config.api_port);

    let rpc_client = Arc::new(HeliusRpcClient::new(RpcClientConfig {
        url: config.helius_rpc_url.clone(),
        rate_limit_rps: config.rpc_rate_limit,
        commitment: CommitmentConfig::confirmed(),
    }));

    tracing::info!(
        "RPC client initialized (rate_limit={} rps)",
        config.rpc_rate_limit
    );

    let idl = load_idl(&config.idl_path, rpc_client.inner(), &config.program_id)
        .await
        .context("Failed to load Anchor IDL")?;

    tracing::info!(
        "IDL loaded: '{}' with {} instructions",
        idl.program_name,
        idl.discriminator_map.len()
    );

    let postgres = Arc::new(
        PostgresDb::connect(&config.postgres_url, config.db_max_connections)
            .await
            .context("Failed to connect to PostgreSQL")?,
    );

    // Run migrations on startup
    postgres
        .run_migrations()
        .await
        .context("Failed to run PostgreSQL migrations")?;

    tracing::info!("Connected to PostgreSQL");

    let clickhouse = Arc::new(ClickhouseDb::new(
        &config.clickhouse_url,
        &config.clickhouse_user,
        &config.clickhouse_password,
        &config.clickhouse_db,
        config.ch_batch_size,
    ));

    clickhouse
        .ping()
        .await
        .context("Failed to connect to ClickHouse")?;

    // Initialize ClickHouse tables
    clickhouse
        .init_tables()
        .await
        .context("Failed to initialize ClickHouse tables")?;

    tracing::info!("Connected to ClickHouse");

    let config = Arc::new(config);
    let idl = Arc::new(idl);

    let api_state = AppState {
        postgres: Arc::clone(&postgres),
        clickhouse: Arc::clone(&clickhouse),
    };

    let api_port = config.api_port;
    let api_handle = tokio::spawn(async move {
        if let Err(e) = serve(api_state, api_port).await {
            tracing::error!(error = %e, "API server crashed");
        }
    });

    tracing::info!("API server spawned on port {}", api_port);

    let shutdown_rx = wait_for_shutdown().await;

    let indexer = Indexer::new(
        Arc::clone(&config),
        Arc::clone(&rpc_client),
        Arc::clone(&postgres),
        Arc::clone(&clickhouse),
        Arc::clone(&idl),
    );

    tracing::info!("Starting indexer loop...");

    indexer
        .run(shutdown_rx)
        .await
        .context("Indexer run failed")?;

    api_handle.abort();

    tracing::info!("Bubblegum shutdown complete");

    Ok(())
}

fn init_logging(log_level: &str) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let is_production = std::env::var("RUST_ENV")
        .map(|v| v == "production")
        .unwrap_or(false);

    if is_production {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty())
            .init();
    }
}
