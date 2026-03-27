mod config;
mod idl;
mod rpc;
mod decoder;
mod db;
mod indexer;
mod api;
mod logging;

use config::Config;
use idl::Idl;
use rpc::RpcService;
use decoder::Decoder;
use db::postgres::PostgresDb;
use db::clickhouse::ClickHouseDb;
use indexer::Indexer;
use api::run_api;
use std::sync::Arc;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    logging::init_logging();

    let config = Config::from_env()?;

    let idl = if let Some(path) = &config.idl_path {
        Idl::from_file(path)?
    } else {
        let rpc_client = solana_rpc_client::nonblocking::rpc_client::RpcClient::new(config.helius_rpc_url.clone());
        Idl::from_account(&rpc_client, &config.program_id).await?
    };

    let decoder = Decoder::new(idl);
    let rpc = RpcService::new(&config.helius_rpc_url);
    let postgres = Arc::new(PostgresDb::new(&config.postgres_url).await?);
    postgres.init().await?;
    let clickhouse = Arc::new(ClickHouseDb::new(&config.clickhouse_url).await?);
    clickhouse.init().await?;

    let indexer = Indexer::new(config.clone(), rpc, decoder, postgres.clone(), clickhouse.clone()).await;

    let api_state = api::AppState {
        postgres: postgres.clone(),
        clickhouse: clickhouse.clone(),
    };

    let api_handle = tokio::spawn(run_api(api_state, 3000));

    let indexer_handle = tokio::spawn(async move {
        if let Err(e) = indexer.run().await {
            tracing::error!("Indexer error: {}", e);
        }
    });

    tokio::select! {
        _ = api_handle => {},
        _ = indexer_handle => {},
        _ = signal::ctrl_c() => {
            tracing::info!("Shutting down...");
        }
    }

    Ok(())
}