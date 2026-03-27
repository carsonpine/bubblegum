use crate::config::Config;
use crate::decoder::{DecodedInstruction, Decoder};
use crate::db::clickhouse::{ClickHouseDb, TransactionHistory};
use crate::db::postgres::PostgresDb;
use crate::rpc::RpcService;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{info, warn, error, debug};

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

        let last_slot = self.postgres.get_checkpoint("last_indexed_slot").await?;
        let start_slot = if let Some(slot) = last_slot {
            slot + 1
        } else if let Some(slot) = self.config.start_slot {
            slot as i64
        } else {
            0
        };

        let end_slot = self.config.end_slot.map(|s| s as i64);

        info!("Indexer starting from slot {}", start_slot);

        let mut current_slot = start_slot;
        let mut processed = 0;
        let mut start_time = Instant::now();

        *self.running.write().await = true;

        while *self.running.read().await {
            let target_slot = if let Some(end) = end_slot {
                if current_slot > end {
                    break;
                }
                end as u64
            } else {
                self.rpc.get_slot().await?
            };

            if current_slot as u64 > target_slot {
                if end_slot.is_none() {
                    sleep(Duration::from_secs(2)).await;
                    continue;
                } else {
                    break;
                }
            }

            let signatures = self.rpc.get_signatures_for_address(
                &program_id,
                None,
                None,
            ).await?;

            if signatures.is_empty() {
                if end_slot.is_none() {
                    sleep(Duration::from_secs(5)).await;
                    continue;
                } else {
                    break;
                }
            }

            let mut batch_txs = Vec::new();
            for sig_info in signatures {
                let sig = sig_info.signature;
                if let Ok(tx) = self.rpc.get_transaction_by_signature(&sig).await {
                    if tx.slot >= current_slot as u64 && tx.slot <= target_slot {
                        batch_txs.push(tx);
                        if batch_txs.len() >= batch_size {
                            self.process_batch(&batch_txs, &program_id).await?;
                            processed += batch_txs.len();
                            batch_txs.clear();

                            if processed % 100 == 0 {
                                let elapsed = start_time.elapsed();
                                let rate = processed as f64 / elapsed.as_secs_f64();
                                info!("Progress: {} txs, rate: {:.1} tx/s", processed, rate);
                            }
                        }
                    }
                }
            }

            if !batch_txs.is_empty() {
                self.process_batch(&batch_txs, &program_id).await?;
                processed += batch_txs.len();
            }

            let last_processed_slot = batch_txs.last().map(|t| t.slot).unwrap_or(current_slot as u64);
            self.postgres.update_checkpoint("last_indexed_slot", last_processed_slot as i64).await?;
            current_slot = (last_processed_slot + 1) as i64;

            if end_slot.is_some() && current_slot > end_slot.unwrap() as i64 {
                break;
            }

            sleep(Duration::from_millis(100)).await;
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
        let mut pg_records = Vec::new();
        let mut ch_records = Vec::new();

        for tx in txs {
            let decoded = self.decoder.decode_transaction(tx, program_id)?;
            for instr in decoded {
                let signature = instr.signature.clone();
                let slot = instr.slot;
                let block_time = instr.timestamp;
                let program_id_str = instr.program_id.clone();
                let signer = instr.signer.clone();
                let instruction_name = instr.instruction_name.clone();
                let args = instr.args.clone();
                let accounts = serde_json::to_value(&instr.accounts)?;

                pg_records.push((signature.clone(), slot, block_time, program_id_str.clone(), signer.clone(),
                                 instruction_name.clone(), args, accounts, None));

                let accounts_vec: Vec<String> = instr.accounts.iter().map(|a| a.pubkey.clone()).collect();
                ch_records.push(TransactionHistory {
                    signature: signature.clone(),
                    slot,
                    block_time,
                    program_id: program_id_str,
                    signer,
                    instruction_name,
                    instruction_args: instr.args.to_string(),
                    accounts: accounts_vec,
                    transaction_hash: signature,
                });
            }
        }

        for (sig, slot, bt, pid, signer, instr, args, accs, _) in pg_records {
            self.postgres.insert_transaction(
                &sig, slot, bt, &pid, &signer, &instr, args, accs, None,
            ).await?;
        }

        self.clickhouse.insert_transactions(&ch_records).await?;

        Ok(())
    }

    pub async fn stop(&self) {
        *self.running.write().await = false;
        info!("Indexer stop signal received");
    }
}