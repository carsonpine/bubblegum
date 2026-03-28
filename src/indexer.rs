use crate::config::Config;
use crate::db::clickhouse::{ClickHouseDb, TransactionHistory};
use crate::db::postgres::PostgresDb;
use crate::decoder::Decoder;
use crate::rpc::RpcService;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, info, warn};

pub struct Indexer {
    config: Config,
    rpc: RpcService,
    decoder: Decoder,
    postgres: Arc<PostgresDb>,
    clickhouse: Arc<ClickHouseDb>,
    running: Arc<RwLock<bool>>,
}

impl Indexer {
    pub async fn new(
        config: Config,
        rpc: RpcService,
        decoder: Decoder,
        postgres: Arc<PostgresDb>,
        clickhouse: Arc<ClickHouseDb>,
    ) -> Self {
        Self {
            config,
            rpc,
            decoder,
            postgres,
            clickhouse,
            running: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn run(&self) -> Result<(), anyhow::Error> {
        let program_id = self.config.program_id;
        let batch_size = self.config.batch_size;
        let end_slot = self.config.end_slot.map(|s| s as i64);

        let last_slot = self.postgres.get_checkpoint("last_indexed_slot").await?;
        let mut from_slot = if let Some(slot) = last_slot {
            slot + 1
        } else if let Some(slot) = self.config.start_slot {
            slot as i64
        } else {
            self.rpc.get_slot().await? as i64
        };

        info!("Indexer starting from slot {}", from_slot);

        let mut processed = 0usize;
        let start_time = Instant::now();

        *self.running.write().await = true;

        'outer: while *self.running.read().await {
            if let Some(end) = end_slot {
                if from_slot > end {
                    break;
                }
            }

            let tip = self.rpc.get_slot().await?;
            let target = end_slot.map(|e| e as u64).unwrap_or(tip);

            if from_slot as u64 > tip {
                sleep(Duration::from_secs(2)).await;
                continue;
            }

            // Paginate signatures newest→oldest, fetch and process each page immediately.
            let mut before: Option<Signature> = None;

            loop {
                if !*self.running.read().await {
                    break 'outer;
                }

                let page = match self
                    .rpc
                    .get_signatures_for_address(&program_id, before, None)
                    .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to fetch signatures: {}", e);
                        sleep(Duration::from_secs(1)).await;
                        break;
                    }
                };

                if page.is_empty() {
                    break;
                }

                before = page.last().map(|s| s.signature);

                // Collect sigs in this page that fall within our range.
                let mut sigs: Vec<Signature> = Vec::new();
                let mut reached_start = false;
                for sig_info in &page {
                    if sig_info.slot < from_slot as u64 {
                        reached_start = true;
                        break;
                    }
                    if sig_info.slot <= target {
                        sigs.push(sig_info.signature);
                    }
                }

                // Process this page oldest-first so the checkpoint advances correctly.
                sigs.reverse();

                let mut batch_txs = Vec::new();
                for sig in sigs {
                    match self.rpc.get_transaction_by_signature(&sig).await {
                        Ok(tx) => {
                            batch_txs.push(tx);
                            if batch_txs.len() >= batch_size {
                                let last_slot = batch_txs.last().map(|t| t.slot).unwrap();
                                self.process_batch(&batch_txs, &program_id).await?;
                                processed += batch_txs.len();
                                self.postgres
                                    .update_checkpoint("last_indexed_slot", last_slot as i64)
                                    .await?;
                                batch_txs.clear();

                                let elapsed = start_time.elapsed();
                                let rate =
                                    processed as f64 / elapsed.as_secs_f64().max(0.001);
                                info!("Progress: {} txs, rate: {:.1} tx/s", processed, rate);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to fetch transaction {}: {}", sig, e);
                        }
                    }
                }

                if !batch_txs.is_empty() {
                    let last_slot = batch_txs.last().map(|t| t.slot).unwrap();
                    self.process_batch(&batch_txs, &program_id).await?;
                    processed += batch_txs.len();
                    self.postgres
                        .update_checkpoint("last_indexed_slot", last_slot as i64)
                        .await?;

                    let elapsed = start_time.elapsed();
                    let rate = processed as f64 / elapsed.as_secs_f64().max(0.001);
                    info!("Progress: {} txs, rate: {:.1} tx/s", processed, rate);
                }

                if reached_start {
                    break;
                }
            }

            from_slot = tip as i64 + 1;
            sleep(Duration::from_millis(500)).await;
        }

        info!("Indexing complete. Processed {} transactions.", processed);
        *self.running.write().await = false;
        Ok(())
    }

    async fn process_batch(
        &self,
        txs: &[EncodedConfirmedTransactionWithStatusMeta],
        program_id: &Pubkey,
    ) -> Result<(), anyhow::Error> {
        let mut ch_records = Vec::new();

        let mut decoded_total = 0usize;
        for tx in txs {
            let decoded = match self.decoder.decode_transaction(tx, program_id) {
                Ok(d) => d,
                Err(crate::decoder::DecodeError::UnknownDiscriminator) => {
                    warn!("Unknown discriminator in tx at slot {}, skipping", tx.slot);
                    continue;
                }
                Err(e) => {
                    warn!("Failed to decode tx at slot {}: {}", tx.slot, e);
                    continue;
                }
            };

            decoded_total += decoded.len();
            for instr in decoded {
                let accounts_json = serde_json::to_value(&instr.accounts)?;
                let accounts_vec: Vec<String> =
                    instr.accounts.iter().map(|a| a.pubkey.clone()).collect();

                self.postgres
                    .insert_transaction(
                        &instr.signature,
                        instr.slot,
                        instr.timestamp,
                        &instr.program_id,
                        &instr.signer,
                        &instr.instruction_name,
                        instr.args.clone(),
                        accounts_json,
                        None,
                    )
                    .await?;

                ch_records.push(TransactionHistory {
                    signature: instr.signature,
                    slot: instr.slot,
                    block_time: instr.timestamp,
                    program_id: instr.program_id,
                    signer: instr.signer,
                    instruction_name: instr.instruction_name,
                    instruction_args: instr.args.to_string(),
                    accounts: accounts_vec,
                });
            }
        }

        info!(
            "Batch: {} fetched, {} instructions decoded, {} CH records",
            txs.len(),
            decoded_total,
            ch_records.len()
        );

        self.clickhouse.insert_transactions(&ch_records).await?;

        Ok(())
    }

    pub async fn stop(&self) {
        *self.running.write().await = false;
        info!("Indexer stop signal received");
    }
}
