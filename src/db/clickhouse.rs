use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};

use crate::decoder::DecodedInstruction;

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TransactionHistoryRow {
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

#[derive(Clone)]
pub struct ClickhouseDb {
    client: Client,
    database: String,
    batch_size: usize,
}

impl ClickhouseDb {
    pub fn new(url: &str, user: &str, password: &str, database: &str, batch_size: usize) -> Self {
        let client = Client::default()
            .with_url(url)
            .with_user(user)
            .with_password(password)
            .with_database(database);

        ClickhouseDb {
            client,
            database: database.to_string(),
            batch_size,
        }
    }

    pub async fn ping(&self) -> Result<()> {
        self.client
            .query("SELECT 1")
            .fetch_one::<u8>()
            .await
            .context("ClickHouse ping failed")?;

        tracing::info!("Connected to ClickHouse (database='{}')", self.database);

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn insert_transaction(&self, decoded: &DecodedInstruction) -> Result<()> {
        self.insert_transactions_batch(std::slice::from_ref(decoded))
            .await
    }

    pub async fn insert_transactions_batch(
        &self,
        transactions: &[DecodedInstruction],
    ) -> Result<()> {
        if transactions.is_empty() {
            return Ok(());
        }

        let chunks: Vec<&[DecodedInstruction]> = transactions.chunks(self.batch_size).collect();

        for chunk in chunks {
            let mut inserter = self
                .client
                .insert("transactions_history")
                .context("Failed to create ClickHouse inserter")?;

            for decoded in chunk {
                let args_str =
                    serde_json::to_string(&decoded.args).unwrap_or_else(|_| "{}".to_string());

                let accounts_str: Vec<String> = decoded
                    .accounts
                    .iter()
                    .map(|a| format!("{}:{}", a.name, a.pubkey))
                    .collect();

                let row = TransactionHistoryRow {
                    signature: decoded.signature.clone(),
                    slot: decoded.slot,
                    block_time: decoded.timestamp,
                    program_id: decoded.program_id.clone(),
                    signer: decoded.signer.clone(),
                    instruction_name: decoded.instruction_name.clone(),
                    instruction_args: args_str,
                    accounts: accounts_str,
                    transaction_hash: decoded.signature.clone(),
                };

                inserter.write(&row).await.with_context(|| {
                    format!(
                        "Failed to write transaction {} to ClickHouse",
                        decoded.signature
                    )
                })?;
            }

            inserter
                .end()
                .await
                .context("Failed to finalize ClickHouse batch insert")?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_instruction_stats(&self) -> Result<Vec<InstructionStatRow>> {
        let rows = self
            .client
            .query(
                r#"
                SELECT
                    instruction_name,
                    date,
                    sum(total_count) AS total_count
                FROM instruction_stats_buffer
                GROUP BY instruction_name, date
                ORDER BY date DESC, total_count DESC
                LIMIT 100
                "#,
            )
            .fetch_all::<InstructionStatRow>()
            .await
            .context("Failed to query instruction stats from ClickHouse")?;

        Ok(rows)
    }

    pub async fn execute_raw_query(&self, sql: &str) -> Result<Vec<serde_json::Value>> {
        let rows: Vec<serde_json::Value> = self
            .client
            .query(sql)
            .fetch_all::<String>()
            .await
            .context("Failed to execute raw ClickHouse query")?
            .into_iter()
            .map(|row_str| {
                serde_json::from_str::<serde_json::Value>(&row_str)
                    .unwrap_or(serde_json::Value::String(row_str))
            })
            .collect();

        Ok(rows)
    }

    pub async fn get_total_count(&self) -> Result<u64> {
        let count: u64 = self
            .client
            .query("SELECT count() FROM transactions_history")
            .fetch_one::<u64>()
            .await
            .context("Failed to count ClickHouse transactions")?;

        Ok(count)
    }

    #[allow(dead_code)]
    pub async fn get_recent_transactions(&self, limit: u64) -> Result<Vec<TransactionHistoryRow>> {
        let rows = self
            .client
            .query(&format!(
                r#"
                SELECT
                    signature,
                    slot,
                    block_time,
                    program_id,
                    signer,
                    instruction_name,
                    instruction_args,
                    accounts,
                    transaction_hash
                FROM transactions_history
                ORDER BY slot DESC
                LIMIT {}
                "#,
                limit
            ))
            .fetch_all::<TransactionHistoryRow>()
            .await
            .context("Failed to fetch recent ClickHouse transactions")?;

        Ok(rows)
    }
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct InstructionStatRow {
    pub instruction_name: String,
    pub date: u32,
    pub total_count: u64,
}
