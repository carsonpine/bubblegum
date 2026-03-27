use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client::rpc_config::{
    RpcTransactionConfig, RpcSignaturesForAddressConfig,
    RpcTransactionDetails, RpcTransactionStatus,
};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta,
    TransactionSignature,
};
use std::time::Duration;
use tokio::time::sleep;
use thiserror::Error;

pub struct RpcService {
    client: RpcClient,
    rate_limit: Duration,
}

impl RpcService {
    pub fn new(url: &str) -> Self {
        let client = RpcClient::new_with_commitment(
            url.to_string(),
            CommitmentConfig::confirmed(),
        );
        Self {
            client,
            rate_limit: Duration::from_millis(50),
        }
    }

    pub async fn get_transactions_by_slot_range(
        &self,
        start_slot: u64,
        end_slot: u64,
        program_id: &Pubkey,
    ) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, RpcError> {
        let mut all_txs = Vec::new();
        for slot in start_slot..=end_slot {
            let block = self.client.get_block(slot).await?;
            for tx in block.transactions {
                if let Some(meta) = &tx.meta {
                    let program_ids = tx
                        .transaction
                        .message
                        .account_keys()
                        .iter()
                        .map(|key| key.to_string())
                        .collect::<Vec<_>>();
                    if program_ids.contains(&program_id.to_string()) {
                        all_txs.push(tx.clone());
                    }
                }
            }
            sleep(self.rate_limit).await;
        }
        Ok(all_txs)
    }

    pub async fn get_transaction_by_signature(
        &self,
        signature: &Signature,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta, RpcError> {
        let config = RpcTransactionConfig {
            encoding: None,
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };
        let tx = self.client.get_transaction_with_config(signature, config).await?;
        Ok(tx)
    }

    pub async fn get_signatures_for_address(
        &self,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> Result<Vec<TransactionSignature>, RpcError> {
        let config = RpcSignaturesForAddressConfig {
            before,
            until,
            commitment: Some(CommitmentConfig::confirmed()),
            limit: Some(1000),
        };
        let sigs = self.client.get_signatures_for_address_with_config(address, config).await?;
        Ok(sigs)
    }

    pub async fn get_slot(&self) -> Result<u64, RpcError> {
        Ok(self.client.get_slot().await?)
    }
}

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("RPC error: {0}")]
    Client(#[from] solana_rpc_client::nonblocking::rpc_client::Error),
}