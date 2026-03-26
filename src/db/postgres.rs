use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::time::Duration;

use crate::decoder::DecodedInstruction;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TransactionRow {
    pub signature: String,
    pub slot: i64,
    pub block_time: Option<i64>,
    pub program_id: String,
    pub signer: String,
    pub instruction_name: String,
    pub instruction_args: Option<serde_json::Value>,
    pub accounts: Option<serde_json::Value>,
    pub raw_transaction: Option<serde_json::Value>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub struct TransactionFilters {
    pub instruction: Option<String>,
    pub signer: Option<String>,
    pub start_slot: Option<i64>,
    pub end_slot: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PostgresDb {
    pool: PgPool,
}

impl PostgresDb {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect(url)
            .await
            .with_context(|| format!("Failed to connect to PostgreSQL at: {}", url))?;

        sqlx::query("SELECT 1")
            .fetch_one(&pool)
            .await
            .context("PostgreSQL health check failed")?;

        tracing::info!("Connected to PostgreSQL (max_connections={})", max_connections);

        Ok(PostgresDb { pool })
    }

    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .context("Failed to run PostgreSQL migrations")?;
        tracing::info!("PostgreSQL migrations applied");
        Ok(())
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn insert_transaction(
        &self,
        decoded: &DecodedInstruction,
        raw_tx_json: Option<serde_json::Value>,
    ) -> Result<()> {
        let accounts_json = serde_json::to_value(&decoded.accounts)
            .context("Failed to serialize decoded accounts")?;

        sqlx::query(
            r#"
            INSERT INTO transactions
                (signature, slot, block_time, program_id, signer, instruction_name,
                 instruction_args, accounts, raw_transaction)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (signature) DO NOTHING
            "#,
        )
        .bind(&decoded.signature)
        .bind(decoded.slot as i64)
        .bind(decoded.timestamp)
        .bind(&decoded.program_id)
        .bind(&decoded.signer)
        .bind(&decoded.instruction_name)
        .bind(&decoded.args)
        .bind(&accounts_json)
        .bind(&raw_tx_json)
        .execute(&self.pool)
        .await
        .with_context(|| {
            format!(
                "Failed to insert transaction {} into PostgreSQL",
                decoded.signature
            )
        })?;

        Ok(())
    }

    pub async fn insert_transactions_batch(
        &self,
        transactions: &[DecodedInstruction],
    ) -> Result<usize> {
        if transactions.is_empty() {
            return Ok(0);
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .context("Failed to begin PostgreSQL transaction")?;

        let mut inserted = 0usize;

        for decoded in transactions {
            let accounts_json = serde_json::to_value(&decoded.accounts)
                .context("Failed to serialize accounts")?;

            let result = sqlx::query(
                r#"
                INSERT INTO transactions
                    (signature, slot, block_time, program_id, signer, instruction_name,
                     instruction_args, accounts, raw_transaction)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (signature) DO NOTHING
                "#,
            )
            .bind(&decoded.signature)
            .bind(decoded.slot as i64)
            .bind(decoded.timestamp)
            .bind(&decoded.program_id)
            .bind(&decoded.signer)
            .bind(&decoded.instruction_name)
            .bind(&decoded.args)
            .bind(&accounts_json)
            .bind(serde_json::Value::Null)
            .execute(&mut *tx)
            .await
            .with_context(|| {
                format!(
                    "Batch insert failed for transaction {}",
                    decoded.signature
                )
            })?;

            inserted += result.rows_affected() as usize;
        }

        tx.commit()
            .await
            .context("Failed to commit batch insert transaction")?;

        Ok(inserted)
    }

    pub async fn get_transaction(&self, signature: &str) -> Result<Option<TransactionRow>> {
        let row = sqlx::query_as::<_, TransactionRow>(
            r#"
            SELECT signature, slot, block_time, program_id, signer, instruction_name,
                   instruction_args, accounts, raw_transaction, created_at
            FROM transactions
            WHERE signature = $1
            "#,
        )
        .bind(signature)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to query transaction {}", signature))?;

        Ok(row)
    }

    pub async fn list_transactions(
        &self,
        filters: &TransactionFilters,
    ) -> Result<Vec<TransactionRow>> {
        let limit = filters.limit.unwrap_or(50).min(500);
        let offset = filters.offset.unwrap_or(0);

        let mut conditions: Vec<String> = Vec::new();
        let mut param_index = 1i32;

        if filters.instruction.is_some() {
            conditions.push(format!("instruction_name = ${}", param_index));
            param_index += 1;
        }
        if filters.signer.is_some() {
            conditions.push(format!("signer = ${}", param_index));
            param_index += 1;
        }
        if filters.start_slot.is_some() {
            conditions.push(format!("slot >= ${}", param_index));
            param_index += 1;
        }
        if filters.end_slot.is_some() {
            conditions.push(format!("slot <= ${}", param_index));
            param_index += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let query_str = format!(
            r#"
            SELECT signature, slot, block_time, program_id, signer, instruction_name,
                   instruction_args, accounts, raw_transaction, created_at
            FROM transactions
            {}
            ORDER BY slot DESC
            LIMIT ${} OFFSET ${}
            "#,
            where_clause, param_index, param_index + 1
        );

        let mut query = sqlx::query_as::<_, TransactionRow>(&query_str);

        if let Some(ref instruction) = filters.instruction {
            query = query.bind(instruction);
        }
        if let Some(ref signer) = filters.signer {
            query = query.bind(signer);
        }
        if let Some(start) = filters.start_slot {
            query = query.bind(start);
        }
        if let Some(end) = filters.end_slot {
            query = query.bind(end);
        }

        query = query.bind(limit).bind(offset);

        let rows = query
            .fetch_all(&self.pool)
            .await
            .context("Failed to list transactions from PostgreSQL")?;

        Ok(rows)
    }

    pub async fn get_checkpoint(&self, key: &str) -> Result<Option<i64>> {
        let row = sqlx::query("SELECT value FROM checkpoints WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .with_context(|| format!("Failed to read checkpoint '{}'", key))?;

        Ok(row.map(|r| r.get::<i64, _>("value")))
    }

    pub async fn set_checkpoint(&self, key: &str, value: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO checkpoints (key, value, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (key) DO UPDATE
                SET value = EXCLUDED.value,
                    updated_at = NOW()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .with_context(|| format!("Failed to set checkpoint '{}' to {}", key, value))?;

        Ok(())
    }

    pub async fn insert_dead_letter(
        &self,
        signature: Option<&str>,
        slot: Option<i64>,
        error_message: &str,
        raw_data: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO dead_letters (signature, slot, error_message, raw_data)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(signature)
        .bind(slot)
        .bind(error_message)
        .bind(raw_data)
        .execute(&self.pool)
        .await
        .context("Failed to insert dead letter")?;

        Ok(())
    }

    pub async fn get_stats(&self) -> Result<DbStats> {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count transactions")?;

        let last_slot: Option<i64> = sqlx::query_scalar("SELECT MAX(slot) FROM transactions")
            .fetch_one(&self.pool)
            .await
            .context("Failed to get max slot")?;

        let checkpoint: Option<i64> = self.get_checkpoint("last_indexed_slot").await?;

        let programs: i64 =
            sqlx::query_scalar("SELECT COUNT(DISTINCT program_id) FROM transactions")
                .fetch_one(&self.pool)
                .await
                .context("Failed to count distinct programs")?;

        Ok(DbStats {
            total_transactions: total,
            last_indexed_slot: last_slot.unwrap_or(0),
            checkpoint_slot: checkpoint.unwrap_or(0),
            programs_indexed: programs,
        })
    }

    pub async fn execute_raw_query(
        &self,
        sql: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(sql)
            .fetch_all(&self.pool)
            .await
            .context("Failed to execute raw SQL query")?;

        let mut results = Vec::new();
        for row in rows {
            let mut map = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                let val: serde_json::Value = row
                    .try_get_raw(i)
                    .ok()
                    .and_then(|v| {
                        if v.is_null() {
                            Some(serde_json::Value::Null)
                        } else {
                            row.try_get::<String, _>(i)
                                .ok()
                                .map(serde_json::Value::String)
                                .or_else(|| {
                                    row.try_get::<i64, _>(i)
                                        .ok()
                                        .map(|n| serde_json::json!(n))
                                })
                                .or_else(|| {
                                    row.try_get::<f64, _>(i)
                                        .ok()
                                        .map(|n| serde_json::json!(n))
                                })
                                .or_else(|| {
                                    row.try_get::<bool, _>(i)
                                        .ok()
                                        .map(serde_json::Value::Bool)
                                })
                                .or_else(|| {
                                    row.try_get::<serde_json::Value, _>(i).ok()
                                })
                        }
                    })
                    .unwrap_or(serde_json::Value::Null);
                map.insert(col.name().to_string(), val);
            }
            results.push(serde_json::Value::Object(map));
        }

        Ok(results)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStats {
    pub total_transactions: i64,
    pub last_indexed_slot: i64,
    pub checkpoint_slot: i64,
    pub programs_indexed: i64,
}