use clickhouse_rs::{Block, ClientHandle, Pool};
use clickhouse_rs::types::{Column, Complex, Row};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ClickHouseDb {
    pool: Pool,
}

#[derive(Debug)]
pub struct TransactionHistory {
    pub signature: String,
    pub slot: u64,
    pub block_time: i64,
    pub program_id: String,
    pub signer: String,
    pub instruction_name: String,
    pub instruction_args: String,
    pub accounts: Vec<String>,
    pub transaction_hash: String,
}

impl ClickHouseDb {
    pub async fn new(url: &str) -> Result<Self, ClickHouseError> {
        let pool = Pool::new(url.parse()?);
        let mut client = pool.get_handle().await?;
        client.ping().await?;
        Ok(Self { pool })
    }

    pub async fn init(&self) -> Result<(), ClickHouseError> {
        let mut client = self.pool.get_handle().await?;

        client.execute(
            r#"
            CREATE TABLE IF NOT EXISTS transactions_history (
                signature String,
                slot UInt64,
                block_time Int64,
                program_id String,
                signer String,
                instruction_name LowCardinality(String),
                instruction_args String,
                accounts Array(String),
                transaction_hash String
            ) ENGINE = MergeTree()
            ORDER BY (program_id, slot, signature)
            "#,
        ).await?;

        client.execute(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS mv_instruction_stats
            ENGINE = SummingMergeTree()
            ORDER BY (instruction_name, date)
            AS SELECT
                instruction_name,
                toStartOfDay(toDateTime(block_time)) AS date,
                count() AS total_count
            FROM transactions_history
            GROUP BY instruction_name, date
            "#,
        ).await?;

        Ok(())
    }

    pub async fn insert_transactions(&self, records: &[TransactionHistory]) -> Result<(), ClickHouseError> {
        if records.is_empty() {
            return Ok(());
        }

        let mut client = self.pool.get_handle().await?;
        let mut block = Block::new();

        block = block
            .column("signature", records.iter().map(|r| r.signature.clone()).collect::<Vec<_>>())
            .column("slot", records.iter().map(|r| r.slot).collect::<Vec<_>>())
            .column("block_time", records.iter().map(|r| r.block_time).collect::<Vec<_>>())
            .column("program_id", records.iter().map(|r| r.program_id.clone()).collect::<Vec<_>>())
            .column("signer", records.iter().map(|r| r.signer.clone()).collect::<Vec<_>>())
            .column("instruction_name", records.iter().map(|r| r.instruction_name.clone()).collect::<Vec<_>>())
            .column("instruction_args", records.iter().map(|r| r.instruction_args.clone()).collect::<Vec<_>>())
            .column("accounts", records.iter().map(|r| r.accounts.clone()).collect::<Vec<_>>())
            .column("transaction_hash", records.iter().map(|r| r.transaction_hash.clone()).collect::<Vec<_>>());

        client.insert("transactions_history", block).await?;

        Ok(())
    }

    pub async fn execute_query(&self, sql: &str) -> Result<Vec<Row>, ClickHouseError> {
        let mut client = self.pool.get_handle().await?;
        let block = client.query(sql).fetch_all().await?;
        Ok(block.rows().collect())
    }

    pub async fn get_total_count(&self) -> Result<u64, ClickHouseError> {
        let mut client = self.pool.get_handle().await?;
        let block = client.query("SELECT count() FROM transactions_history").fetch_all().await?;
        if let Some(row) = block.rows().next() {
            let count: u64 = row.get(0)?;
            Ok(count)
        } else {
            Ok(0)
        }
    }
}

#[derive(Debug, Error)]
pub enum ClickHouseError {
    #[error("clickhouse error: {0}")]
    Driver(#[from] clickhouse_rs::errors::Error),
    #[error("url parse error: {0}")]
    UrlParse(#[from] url::ParseError),
}