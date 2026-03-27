use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::types::Json;
use serde_json::Value;
use chrono::{DateTime, Utc};
use thiserror::Error;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PostgresDb {
    pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct TransactionRecord {
    pub signature: String,
    pub slot: i64,
    pub block_time: i64,
    pub program_id: String,
    pub signer: String,
    pub instruction_name: String,
    pub instruction_args: Json<Value>,
    pub accounts: Json<Value>,
    pub raw_transaction: Option<Json<Value>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct Checkpoint {
    pub key: String,
    pub value: i64,
    pub updated_at: DateTime<Utc>,
}

impl PostgresDb {
    pub async fn new(url: &str) -> Result<Self, PostgresError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn init(&self) -> Result<(), PostgresError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS transactions (
                signature VARCHAR(88) PRIMARY KEY,
                slot BIGINT NOT NULL,
                block_time BIGINT,
                program_id VARCHAR(44) NOT NULL,
                signer VARCHAR(44) NOT NULL,
                instruction_name VARCHAR(255) NOT NULL,
                instruction_args JSONB,
                accounts JSONB,
                raw_transaction JSONB,
                created_at TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_transactions_signer ON transactions(signer);
            CREATE INDEX IF NOT EXISTS idx_transactions_instruction ON transactions(instruction_name);
            CREATE INDEX IF NOT EXISTS idx_transactions_slot ON transactions(slot);
            CREATE INDEX IF NOT EXISTS idx_transactions_created ON transactions(created_at);
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS checkpoints (
                key TEXT PRIMARY KEY,
                value BIGINT NOT NULL,
                updated_at TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_transaction(
        &self,
        signature: &str,
        slot: u64,
        block_time: i64,
        program_id: &str,
        signer: &str,
        instruction_name: &str,
        instruction_args: Value,
        accounts: Value,
        raw_transaction: Option<Value>,
    ) -> Result<(), PostgresError> {
        sqlx::query(
            r#"
            INSERT INTO transactions (
                signature, slot, block_time, program_id, signer,
                instruction_name, instruction_args, accounts, raw_transaction
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (signature) DO UPDATE SET
                slot = EXCLUDED.slot,
                block_time = EXCLUDED.block_time,
                program_id = EXCLUDED.program_id,
                signer = EXCLUDED.signer,
                instruction_name = EXCLUDED.instruction_name,
                instruction_args = EXCLUDED.instruction_args,
                accounts = EXCLUDED.accounts,
                raw_transaction = EXCLUDED.raw_transaction
            "#,
        )
        .bind(signature)
        .bind(slot as i64)
        .bind(block_time)
        .bind(program_id)
        .bind(signer)
        .bind(instruction_name)
        .bind(Json(instruction_args))
        .bind(Json(accounts))
        .bind(raw_transaction.map(Json))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_checkpoint(&self, key: &str) -> Result<Option<i64>, PostgresError> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT value FROM checkpoints WHERE key = $1
            "#,
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(v,)| v))
    }

    pub async fn update_checkpoint(&self, key: &str, value: i64) -> Result<(), PostgresError> {
        sqlx::query(
            r#"
            INSERT INTO checkpoints (key, value, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (key) DO UPDATE SET
                value = EXCLUDED.value,
                updated_at = NOW()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_transaction(&self, signature: &str) -> Result<Option<TransactionRecord>, PostgresError> {
        let record = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT signature, slot, block_time, program_id, signer, instruction_name,
                   instruction_args, accounts, raw_transaction, created_at
            FROM transactions
            WHERE signature = $1
            "#,
        )
        .bind(signature)
        .fetch_optional(&self.pool)
        .await?;

        Ok(record)
    }

    pub async fn list_transactions(
        &self,
        instruction: Option<&str>,
        signer: Option<&str>,
        start_slot: Option<u64>,
        end_slot: Option<u64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TransactionRecord>, PostgresError> {
        use sqlx::QueryBuilder;

        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT signature, slot, block_time, program_id, signer, instruction_name,
                    instruction_args, accounts, raw_transaction, created_at
            FROM transactions WHERE 1=1"
        );

        if let Some(inst) = instruction {
            builder.push(" AND instruction_name = ");
            builder.push_bind(inst);
        }
        if let Some(sig) = signer {
            builder.push(" AND signer = ");
            builder.push_bind(sig);
        }
        if let Some(start) = start_slot {
            builder.push(" AND slot >= ");
            builder.push_bind(start as i64);
        }
        if let Some(end) = end_slot {
            builder.push(" AND slot <= ");
            builder.push_bind(end as i64);
        }

        builder.push(" ORDER BY slot DESC LIMIT ");
        builder.push_bind(limit);
        builder.push(" OFFSET ");
        builder.push_bind(offset);

        let records = builder.build_query_as::<TransactionRecord>()
            .fetch_all(&self.pool)
            .await?;
        Ok(records)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[derive(Debug, Error)]
pub enum PostgresError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
}