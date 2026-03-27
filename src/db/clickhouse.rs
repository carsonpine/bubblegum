use clickhouse::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ClickHouseDb {
    client: Client,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
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
        // Parse URL and extract credentials if present.
        // For simplicity, we use default user/password and database.
        let client = Client::default()
            .with_url(url)
            .with_user("indexer")
            .with_password("changeme")
            .with_database("solana_indexer");
        Ok(Self { client })
    }

    pub async fn init(&self) -> Result<(), ClickHouseError> {
        self.client.query(
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
        ).execute().await?;

        self.client.query(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS instruction_stats_buffer
            ENGINE = SummingMergeTree()
            ORDER BY (instruction_name, date)
            AS SELECT
                instruction_name,
                toStartOfDay(toDateTime(block_time)) AS date,
                count() AS total_count
            FROM transactions_history
            GROUP BY instruction_name, date
            "#,
        ).execute().await?;

        Ok(())
    }

    pub async fn insert_transactions(&self, records: &[TransactionHistory]) -> Result<(), ClickHouseError> {
        if records.is_empty() {
            return Ok(());
        }

        let mut insert = self.client.insert("transactions_history")?;
        for record in records {
            insert.write(record).await?;
        }
        insert.end().await?;
        Ok(())
    }

    pub async fn execute_query(&self, sql: &str) -> Result<Vec<serde_json::Value>, ClickHouseError> {
        let rows = self.client.query(sql).fetch_all::<serde_json::Value>().await?;
        Ok(rows)
    }

    pub async fn get_total_count(&self) -> Result<u64, ClickHouseError> {
        let count: u64 = self.client
            .query("SELECT count() FROM transactions_history")
            .fetch_one()
            .await?;
        Ok(count)
    }
}

#[derive(Debug, Error)]
pub enum ClickHouseError {
    #[error("clickhouse error: {0}")]
    Client(#[from] clickhouse::error::Error),
}