use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_rpc_client_api::client_error::Error as RpcClientError;
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding};
use std::str::FromStr;
use std::time::Duration;
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

    pub async fn get_transaction_by_signature(
        &self,
        signature: &Signature,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta, RpcError> {
        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        };
        let tx = self
            .client
            .get_transaction_with_config(signature, config)
            .await?;
        Ok(tx)
    }

    pub async fn get_signatures_for_address(
        &self,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> Result<Vec<Signature>, RpcError> {
        let config = GetConfirmedSignaturesForAddress2Config {
            before,
            until,
            commitment: Some(CommitmentConfig::confirmed()),
            limit: Some(1000),
        };
        let sigs = self
            .client
            .get_signatures_for_address_with_config(address, config)
            .await?;
        let signatures = sigs
            .into_iter()
            .map(|s| Signature::from_str(&s.signature))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(signatures)
    }

    pub async fn get_slot(&self) -> Result<u64, RpcError> {
        Ok(self.client.get_slot().await?)
    }
}

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("RPC error: {0}")]
    Client(#[from] RpcClientError),
    #[error("Signature parse error: {0}")]
    SignatureParse(#[from] solana_sdk::signature::ParseSignatureError),
}