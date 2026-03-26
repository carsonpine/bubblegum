use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::signal;
use tokio::sync::watch;

use crate::config::Config;
use crate::db::{ClickhouseDb, PostgresDb};
use crate::decoder::TransactionDecoder;
use crate::idl::ParsedIdl;
use crate::rpc::HeliusRpcClient;

const CHECKPOINT_KEY: &str = "last_indexed_slot";
const PROGRESS_LOG_INTERVAL: usize = 100;

pub struct Indexer {
    config: Arc<Config>,
    rpc: Arc<HeliusRpcClient>,
    postgres: Arc<PostgresDb>,
    clickhouse: Arc<ClickhouseDb>,
    idl: Arc<ParsedIdl>,
    decoder: Arc<TransactionDecoder>,
}

impl Indexer {
    pub fn new(
        config: Arc<Config>,
        rpc: Arc<HeliusRpcClient>,
        postgres: Arc<PostgresDb>,
        clickhouse: Arc<ClickhouseDb>,
        idl: Arc<ParsedIdl>,
    ) -> Self {
        let decoder = Arc::new(TransactionDecoder::new(
            Arc::clone(&idl),
            config.program_id_str.clone(),
        ));

        Indexer {
            config,
            rpc,
            postgres,
            clickhouse,
            idl,
            decoder,
        }
    }

    pub async fn run(&self, shutdown_rx: watch::Receiver<bool>) -> Result<()> {
        tracing::info!(
            "Indexer starting for program: {}",
            self.config.program_id_str
        );

        let current_slot = self
            .rpc
            .get_slot()
            .await
            .context("Failed to get current slot from RPC")?;

        tracing::info!("Current chain slot: {}", current_slot);

        let (start_slot, end_slot) = self.resolve_slot_range(current_slot).await?;

        tracing::info!("Indexing slot range: {} -> {}", start_slot, end_slot);

        let total_slots = end_slot.saturating_sub(start_slot);
        let mut total_transactions_stored = 0usize;
        let mut current_start = start_slot;
        let index_start_time = Instant::now();

        while current_start < end_slot {
            if *shutdown_rx.borrow() {
                tracing::info!("Shutdown signal received, stopping indexer gracefully");
                break;
            }

            let batch_end = (current_start + self.config.batch_size as u64).min(end_slot);

            tracing::debug!("Processing slot batch: {} -> {}", current_start, batch_end);

            match self.process_slot_batch(current_start, batch_end).await {
                Ok(stored) => {
                    total_transactions_stored += stored;
                    self.postgres
                        .set_checkpoint(CHECKPOINT_KEY, batch_end as i64)
                        .await
                        .with_context(|| {
                            format!("Failed to save checkpoint at slot {}", batch_end)
                        })?;

                    let slots_done = current_start.saturating_sub(start_slot);
                    let progress_pct = if total_slots > 0 {
                        (slots_done as f64 / total_slots as f64) * 100.0
                    } else {
                        100.0
                    };

                    let elapsed = index_start_time.elapsed().as_secs_f64();
                    let tx_per_sec = if elapsed > 0.0 {
                        total_transactions_stored as f64 / elapsed
                    } else {
                        0.0
                    };

                    if total_transactions_stored % PROGRESS_LOG_INTERVAL == 0 || stored > 0 {
                        tracing::info!(
                            "Progress: slot {}/{} ({:.1}%) | {} tx stored | {:.1} tx/s",
                            current_start,
                            end_slot,
                            progress_pct,
                            total_transactions_stored,
                            tx_per_sec
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        slot_start = current_start,
                        slot_end = batch_end,
                        error = %e,
                        "Slot batch processing failed, inserting into dead-letter and continuing"
                    );

                    let _ = self
                        .postgres
                        .insert_dead_letter(None, Some(current_start as i64), &e.to_string(), None)
                        .await;
                }
            }

            current_start = batch_end;
        }

        let elapsed = index_start_time.elapsed();
        tracing::info!(
            "Indexing complete! Stored {} transactions in {:.2}s",
            total_transactions_stored,
            elapsed.as_secs_f64()
        );

        Ok(())
    }

    async fn process_slot_batch(&self, start_slot: u64, end_slot: u64) -> Result<usize> {
        let transactions = self
            .rpc
            .get_transactions_by_slot_range(start_slot, end_slot, &self.config.program_id)
            .await
            .with_context(|| {
                format!(
                    "RPC fetch failed for slot range {} -> {}",
                    start_slot, end_slot
                )
            })?;

        if transactions.is_empty() {
            return Ok(0);
        }

        let mut all_decoded = Vec::new();

        for tx in &transactions {
            let slot = tx.slot;
            let signature = extract_signature(tx);

            match self.decoder.decode_transaction(tx) {
                Ok(decoded_instructions) => {
                    for decoded in decoded_instructions {
                        all_decoded.push(decoded);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        signature = %signature,
                        slot = slot,
                        error = %e,
                        "Failed to decode transaction, writing to dead-letter queue"
                    );

                    let _ = self
                        .postgres
                        .insert_dead_letter(
                            Some(&signature),
                            Some(slot as i64),
                            &e.to_string(),
                            None,
                        )
                        .await;
                }
            }
        }

        if all_decoded.is_empty() {
            return Ok(0);
        }

        let pg_inserted = self
            .postgres
            .insert_transactions_batch(&all_decoded)
            .await
            .context("PostgreSQL batch insert failed")?;

        self.clickhouse
            .insert_transactions_batch(&all_decoded)
            .await
            .context("ClickHouse batch insert failed")?;

        tracing::debug!(
            "Batch stored: {} transactions (slots {} -> {})",
            pg_inserted,
            start_slot,
            end_slot
        );

        Ok(pg_inserted)
    }

    async fn resolve_slot_range(&self, current_slot: u64) -> Result<(u64, u64)> {
        let end_slot = self.config.end_slot.unwrap_or(current_slot);

        let start_slot = if let Some(configured_start) = self.config.start_slot {
            configured_start
        } else {
            let checkpoint = self
                .postgres
                .get_checkpoint(CHECKPOINT_KEY)
                .await
                .context("Failed to read indexer checkpoint")?;

            match checkpoint {
                Some(slot) if slot > 0 => {
                    tracing::info!("Resuming from checkpoint slot: {}", slot);
                    slot as u64
                }
                _ => {
                    tracing::info!(
                        "No checkpoint found, starting from slot {} (current - 1000)",
                        current_slot.saturating_sub(1000)
                    );
                    current_slot.saturating_sub(1000)
                }
            }
        };

        Ok((start_slot, end_slot))
    }
}

fn extract_signature(
    tx: &solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta,
) -> String {
    use solana_transaction_status::EncodedTransaction;

    match &tx.transaction.transaction {
        EncodedTransaction::Json(ui_tx) => ui_tx
            .signatures
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        _ => "unknown".to_string(),
    }
}

pub async fn wait_for_shutdown() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);

    tokio::spawn(async move {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        tracing::info!("Shutdown signal received");
        let _ = tx.send(true);
    });

    rx
}
