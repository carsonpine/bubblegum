use anyhow::{anyhow, Context, Result};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout};

const MAX_SIGNATURES_PER_REQUEST: usize = 1000;
const BASE_BACKOFF_MS: u64 = 200;
const MAX_BACKOFF_MS: u64 = 30_000;
const MAX_RETRIES: u32 = 8;
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    pub url: String,
    pub rate_limit_rps: u32,
    pub commitment: CommitmentConfig,
}

pub struct HeliusRpcClient {
    inner: Arc<RpcClient>,
    semaphore: Arc<Semaphore>,
    config: RpcClientConfig,
}

impl HeliusRpcClient {
    pub fn new(config: RpcClientConfig) -> Self {
        let inner = RpcClient::new_with_commitment(config.url.clone(), config.commitment);

        let permits = config.rate_limit_rps.max(1) as usize;
        let semaphore = Arc::new(Semaphore::new(permits));

        HeliusRpcClient {
            inner: Arc::new(inner),
            semaphore,
            config,
        }
    }

    pub fn inner(&self) -> &RpcClient {
        &self.inner
    }

    async fn acquire_rate_limit(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.semaphore.acquire().await.expect("semaphore closed")
    }

    pub async fn get_slot(&self) -> Result<u64> {
        self.with_retry("get_slot", || async {
            let _permit = self.acquire_rate_limit().await;
            timeout(RPC_TIMEOUT, self.inner.get_slot())
                .await
                .map_err(|_| anyhow!("get_slot timed out after {}s", RPC_TIMEOUT.as_secs()))?
                .context("Failed to get current slot")
        })
        .await
    }

    pub async fn get_signatures_for_address(
        &self,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
        limit: Option<usize>,
    ) -> Result<Vec<solana_rpc_client_api::response::RpcConfirmedTransactionStatusWithSignature>>
    {
        self.with_retry("get_signatures_for_address", || async {
            let _permit = self.acquire_rate_limit().await;

            let config = GetConfirmedSignaturesForAddress2Config {
                before,
                until,
                limit: Some(limit.unwrap_or(MAX_SIGNATURES_PER_REQUEST)),
                commitment: Some(self.config.commitment),
            };

            timeout(
                RPC_TIMEOUT,
                self.inner
                    .get_signatures_for_address_with_config(address, config),
            )
            .await
            .map_err(|_| {
                anyhow!(
                    "get_signatures_for_address timed out after {}s",
                    RPC_TIMEOUT.as_secs()
                )
            })?
            .context("Failed to get signatures for address")
        })
        .await
    }

    pub async fn get_transaction(
        &self,
        signature: &Signature,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta> {
        self.with_retry("get_transaction", || async {
            let _permit = self.acquire_rate_limit().await;

            let config = RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                commitment: Some(self.config.commitment),
                max_supported_transaction_version: Some(0),
            };

            timeout(
                RPC_TIMEOUT,
                self.inner.get_transaction_with_config(signature, config),
            )
            .await
            .map_err(|_| {
                anyhow!(
                    "get_transaction timed out after {}s for sig {}",
                    RPC_TIMEOUT.as_secs(),
                    signature
                )
            })?
            .with_context(|| format!("Failed to fetch transaction {}", signature))
        })
        .await
    }

    pub async fn get_transactions_by_slot_range(
        &self,
        start_slot: u64,
        end_slot: u64,
        program_id: &Pubkey,
    ) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>> {
        tracing::info!(
            "Fetching transactions for program {} in slot range [{}, {}]",
            program_id,
            start_slot,
            end_slot
        );

        let all_signatures = self
            .collect_all_signatures_in_range(program_id, start_slot, end_slot)
            .await?;

        tracing::info!(
            "Found {} signatures in slot range [{}, {}]",
            all_signatures.len(),
            start_slot,
            end_slot
        );

        let mut transactions = Vec::with_capacity(all_signatures.len());
        let mut failed = 0usize;

        for signature_str in &all_signatures {
            let signature = match Signature::from_str(signature_str) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Invalid signature '{}': {}", signature_str, e);
                    failed += 1;
                    continue;
                }
            };

            match self.get_transaction(&signature).await {
                Ok(tx) => {
                    transactions.push(tx);
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch transaction {}: {}", signature_str, e);
                    failed += 1;
                }
            }
        }

        if failed > 0 {
            tracing::warn!(
                "Failed to fetch {}/{} transactions in range",
                failed,
                all_signatures.len()
            );
        }

        Ok(transactions)
    }

    async fn collect_all_signatures_in_range(
        &self,
        program_id: &Pubkey,
        start_slot: u64,
        end_slot: u64,
    ) -> Result<Vec<String>> {
        let mut all_signatures: Vec<String> = Vec::new();
        let mut before: Option<Signature> = None;

        loop {
            let batch = self
                .get_signatures_for_address(
                    program_id,
                    before,
                    None,
                    Some(MAX_SIGNATURES_PER_REQUEST),
                )
                .await?;

            if batch.is_empty() {
                break;
            }

            let mut reached_start = false;

            for sig_info in &batch {
                let slot = sig_info.slot;

                if slot > end_slot {
                    continue;
                }

                if slot < start_slot {
                    reached_start = true;
                    break;
                }

                if sig_info.err.is_none() {
                    all_signatures.push(sig_info.signature.clone());
                }
            }

            if reached_start || batch.len() < MAX_SIGNATURES_PER_REQUEST {
                break;
            }

            let last_sig_str = &batch.last().unwrap().signature;
            before = Some(
                Signature::from_str(last_sig_str)
                    .with_context(|| format!("Invalid last signature: {}", last_sig_str))?,
            );
        }

        Ok(all_signatures)
    }

    async fn with_retry<F, Fut, T>(&self, operation: &str, mut f: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0u32;

        loop {
            match f().await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    attempt += 1;

                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "Operation '{}' failed after {} retries. Last error: {}",
                            operation,
                            MAX_RETRIES,
                            err
                        ));
                    }

                    let is_rate_limit = err.to_string().contains("429")
                        || err.to_string().to_lowercase().contains("rate limit")
                        || err.to_string().to_lowercase().contains("too many requests");

                    let is_transient = is_rate_limit
                        || err.to_string().contains("503")
                        || err.to_string().contains("502")
                        || err.to_string().contains("timeout")
                        || err.to_string().contains("timed out")
                        || err.to_string().to_lowercase().contains("connection");

                    if !is_transient {
                        return Err(err
                            .context(format!("Non-retryable error in operation '{}'", operation)));
                    }

                    let backoff_ms = (BASE_BACKOFF_MS * 2u64.pow(attempt - 1)).min(MAX_BACKOFF_MS);
                    let jitter_ms = rand_jitter(backoff_ms / 4);
                    let wait_ms = backoff_ms + jitter_ms;

                    tracing::warn!(
                        operation = operation,
                        attempt = attempt,
                        wait_ms = wait_ms,
                        error = %err,
                        "RPC request failed, retrying with backoff"
                    );

                    sleep(Duration::from_millis(wait_ms)).await;
                }
            }
        }
    }
}

fn rand_jitter(max_ms: u64) -> u64 {
    if max_ms == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos as u64) % max_ms
}

#[allow(dead_code)]
pub fn parse_signature(s: &str) -> Result<Signature> {
    Signature::from_str(s).with_context(|| format!("Invalid Solana signature: '{}'", s))
}
