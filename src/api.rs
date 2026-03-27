use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;

use crate::db::clickhouse::ClickHouseDb;
use crate::db::postgres::PostgresDb;

#[derive(Clone)]
pub struct AppState {
    pub postgres: Arc<PostgresDb>,
    pub clickhouse: Arc<ClickHouseDb>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionFilters {
    instruction: Option<String>,
    signer: Option<String>,
    start_slot: Option<u64>,
    end_slot: Option<u64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TransactionResponse {
    pub signature: String,
    pub slot: u64,
    pub timestamp: i64,
    pub program_id: String,
    pub instruction: serde_json::Value,
    pub signer: String,
    pub accounts: serde_json::Value,
}

impl From<crate::db::postgres::TransactionRecord> for TransactionResponse {
    fn from(record: crate::db::postgres::TransactionRecord) -> Self {
        Self {
            signature: record.signature,
            slot: record.slot as u64,
            timestamp: record.block_time,
            program_id: record.program_id,
            instruction: record.instruction_args.0,
            signer: record.signer,
            accounts: record.accounts.0,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SqlQueryResponse {
    columns: Vec<String>,
    rows: Vec<serde_json::Value>,
    row_count: usize,
    execution_time_ms: u64,
}

pub async fn run_api(state: AppState, port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let static_files = ServeDir::new("static");

    let app = Router::new()
        .route("/api/transaction/:signature", get(get_transaction))
        .route("/api/transactions", get(list_transactions))
        .route("/api/sql", get(execute_sql))
        .route("/api/stats", get(get_stats))
        .nest_service("/", static_files)
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("API server listening on {}", addr);
    axum::Server::bind(&addr.parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn get_transaction(
    Path(signature): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<TransactionResponse>, (StatusCode, String)> {
    let record = state.postgres.get_transaction(&signature)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match record {
        Some(r) => Ok(Json(r.into())),
        None => Err((StatusCode::NOT_FOUND, "Transaction not found".to_string())),
    }
}

async fn list_transactions(
    Query(filters): Query<TransactionFilters>,
    State(state): State<AppState>,
) -> Result<Json<Vec<TransactionResponse>>, (StatusCode, String)> {
    let limit = filters.limit.unwrap_or(50).min(1000);
    let offset = filters.offset.unwrap_or(0);

    let records = state.postgres.list_transactions(
        filters.instruction.as_deref(),
        filters.signer.as_deref(),
        filters.start_slot,
        filters.end_slot,
        limit,
        offset,
    ).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(records.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Deserialize)]
struct SqlQuery {
    db: String,
    sql: String,
}

async fn execute_sql(
    Query(query): Query<SqlQuery>,
    State(state): State<AppState>,
) -> Result<Json<SqlQueryResponse>, (StatusCode, String)> {
    let start = std::time::Instant::now();

    let rows = match query.db.as_str() {
        "postgres" => {
            let rows = sqlx::query(&query.sql)
                .fetch_all(&state.postgres.pool)
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            rows.into_iter()
                .map(|row| {
                    let mut obj = serde_json::Map::new();
                    for (i, col) in row.columns().iter().enumerate() {
                        let value: Result<serde_json::Value, _> = row.try_get(i);
                        if let Ok(v) = value {
                            obj.insert(col.name().to_string(), v);
                        } else {
                            obj.insert(col.name().to_string(), serde_json::Value::Null);
                        }
                    }
                    serde_json::Value::Object(obj)
                })
                .collect()
        }
        "clickhouse" => {
            let rows = state.clickhouse.execute_query(&query.sql).await
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            rows
        }
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid database".to_string())),
    };

    let elapsed = start.elapsed().as_millis() as u64;
    let columns = if let Some(first) = rows.first() {
        if let Some(obj) = first.as_object() {
            obj.keys().cloned().collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    Ok(Json(SqlQueryResponse {
        columns,
        rows,
        row_count: rows.len(),
        execution_time_ms: elapsed,
    }))
}

async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pg_total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.postgres.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let ch_total = state.clickhouse.get_total_count().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let last_slot = state.postgres.get_checkpoint("last_indexed_slot").await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or(0);

    let program_ids: Vec<String> = sqlx::query_scalar("SELECT DISTINCT program_id FROM transactions LIMIT 10")
        .fetch_all(&state.postgres.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "total_transactions": pg_total,
        "last_indexed_slot": last_slot,
        "programs": program_ids,
        "clickhouse_total": ch_total,
    })))
}