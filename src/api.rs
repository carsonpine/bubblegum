use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};

use crate::db::postgres::{DbStats, TransactionFilters};
use crate::db::{ClickhouseDb, PostgresDb};

#[derive(Clone)]
pub struct AppState {
    pub postgres: Arc<PostgresDb>,
    pub clickhouse: Arc<ClickhouseDb>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResponse {
    pub signature: String,
    pub slot: i64,
    pub timestamp: Option<i64>,
    pub program_id: String,
    pub instruction: InstructionResponse,
    pub signer: String,
    pub accounts: serde_json::Value,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstructionResponse {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ListTransactionsQuery {
    pub instruction: Option<String>,
    pub signer: Option<String>,
    pub start_slot: Option<i64>,
    pub end_slot: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SqlQueryRequest {
    pub sql: String,
    pub database: String,
}

#[derive(Debug, Serialize)]
pub struct SqlQueryResponse {
    pub rows: Vec<serde_json::Value>,
    pub row_count: usize,
    pub execution_time_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub postgres: DbStats,
    pub clickhouse_total: u64,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        (status, Json(self)).into_response()
    }
}

fn map_err(e: anyhow::Error) -> (StatusCode, Json<ErrorResponse>) {
    tracing::error!(error = %e, "API handler error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: e.to_string(),
        }),
    )
}

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/transaction/:signature", get(get_transaction))
        .route("/transactions", get(list_transactions))
        .route("/stats", get(get_stats))
        .route("/query", post(execute_sql_query))
        .route("/health", get(health_check))
        .nest_service("/", ServeDir::new("static"))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn get_transaction(
    Path(signature): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<TransactionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let row = state
        .postgres
        .get_transaction(&signature)
        .await
        .map_err(map_err)?;

    match row {
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Transaction '{}' not found", signature),
            }),
        )),
        Some(row) => {
            let response = TransactionResponse {
                signature: row.signature,
                slot: row.slot,
                timestamp: row.block_time,
                program_id: row.program_id,
                instruction: InstructionResponse {
                    name: row.instruction_name,
                    args: row.instruction_args.unwrap_or(serde_json::Value::Null),
                },
                signer: row.signer,
                accounts: row.accounts.unwrap_or(serde_json::Value::Array(vec![])),
                created_at: row.created_at.map(|dt| dt.to_rfc3339()),
            };
            Ok(Json(response))
        }
    }
}

async fn list_transactions(
    Query(params): Query<ListTransactionsQuery>,
    State(state): State<AppState>,
) -> Result<Json<Vec<TransactionResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let filters = TransactionFilters {
        instruction: params.instruction,
        signer: params.signer,
        start_slot: params.start_slot,
        end_slot: params.end_slot,
        limit: params.limit,
        offset: params.offset,
    };

    let rows = state
        .postgres
        .list_transactions(&filters)
        .await
        .map_err(map_err)?;

    let responses: Vec<TransactionResponse> = rows
        .into_iter()
        .map(|row| TransactionResponse {
            signature: row.signature,
            slot: row.slot,
            timestamp: row.block_time,
            program_id: row.program_id,
            instruction: InstructionResponse {
                name: row.instruction_name,
                args: row.instruction_args.unwrap_or(serde_json::Value::Null),
            },
            signer: row.signer,
            accounts: row.accounts.unwrap_or(serde_json::Value::Array(vec![])),
            created_at: row.created_at.map(|dt| dt.to_rfc3339()),
        })
        .collect();

    Ok(Json(responses))
}

async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let pg_stats = state.postgres.get_stats().await.map_err(map_err)?;

    let ch_total = state.clickhouse.get_total_count().await.unwrap_or(0);

    Ok(Json(StatsResponse {
        postgres: pg_stats,
        clickhouse_total: ch_total,
    }))
}

async fn execute_sql_query(
    State(state): State<AppState>,
    Json(payload): Json<SqlQueryRequest>,
) -> Result<Json<SqlQueryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let sql = payload.sql.trim();

    if sql.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "SQL query cannot be empty".to_string(),
            }),
        ));
    }

    let sql_upper = sql.to_uppercase();
    let is_write = sql_upper.starts_with("INSERT")
        || sql_upper.starts_with("UPDATE")
        || sql_upper.starts_with("DELETE")
        || sql_upper.starts_with("DROP")
        || sql_upper.starts_with("TRUNCATE")
        || sql_upper.starts_with("ALTER")
        || sql_upper.starts_with("CREATE");

    if is_write {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Only SELECT queries are permitted via the SQL tool".to_string(),
            }),
        ));
    }

    let start = std::time::Instant::now();

    let rows = match payload.database.to_lowercase().as_str() {
        "clickhouse" | "ch" => state
            .clickhouse
            .execute_raw_query(sql)
            .await
            .map_err(map_err)?,
        "postgres" | "pg" | "postgresql" => state
            .postgres
            .execute_raw_query(sql)
            .await
            .map_err(map_err)?,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!(
                        "Unknown database '{}'. Valid values: 'postgres', 'clickhouse'",
                        other
                    ),
                }),
            ));
        }
    };

    let execution_time_ms = start.elapsed().as_millis();
    let row_count = rows.len();

    Ok(Json(SqlQueryResponse {
        rows,
        row_count,
        execution_time_ms,
    }))
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "bubblegum-indexer"
    }))
}

pub async fn serve(state: AppState, port: u16) -> Result<()> {
    let app = build_router(state);
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind API server to {}: {}", addr, e))?;

    tracing::info!("API server listening on http://{}", addr);

    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("API server error: {}", e))?;

    Ok(())
}
