use serde::Deserialize;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use std::collections::HashMap;
use std::fs;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct Idl {
    pub address: String,
    pub metadata: IdlMetadata,
    pub instructions: Vec<IdlInstruction>,
    pub accounts: Vec<IdlAccount>,
    #[serde(default)]
    pub types: Option<Vec<IdlTypeDef>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlMetadata {
    pub name: String,
    pub version: String,
    pub spec: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlInstruction {
    pub name: String,
    pub discriminator: Vec<u8>,
    pub args: Vec<IdlField>,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: IdlType,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdlAccountItem {
    Single(String),
    Detailed(IdlAccountItemDetailed),
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlAccountItemDetailed {
    pub name: String,
    pub is_mut: bool,
    pub is_signer: bool,
    #[serde(rename = "pda")]
    pub is_pda: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlAccount {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: IdlType,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlTypeDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: IdlTypeDefKind,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdlTypeDefKind {
    Struct {
        fields: Vec<IdlField>,
    },
    Enum {
        variants: Vec<IdlEnumVariant>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlEnumVariant {
    pub name: String,
    #[serde(default)]
    pub fields: Option<Vec<IdlField>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdlType {
    Simple(String),
    Array { array: Vec<IdlType>, size: usize },
    Option(Box<IdlType>),
    Defined(String),
    Vec(Box<IdlType>),
}

impl Idl {
    pub fn from_file(path: &str) -> Result<Self, IdlError> {
        let content = fs::read_to_string(path)?;
        let idl: Idl = serde_json::from_str(&content)?;
        Ok(idl)
    }

    pub async fn from_account(client: &RpcClient, program_id: &Pubkey) -> Result<Self, IdlError> {
        let (pda, _) = Pubkey::find_program_address(&[b"anchor:idl"], program_id);
        let account = client.get_account_data(&pda).await?;
        let idl: Idl = serde_json::from_slice(&account)?;
        Ok(idl)
    }

    pub fn build_discriminator_map(&self) -> HashMap<Vec<u8>, IdlInstruction> {
        self.instructions
            .iter()
            .map(|ix| (ix.discriminator.clone(), ix.clone()))
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum IdlError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("RPC error: {0}")]
    Rpc(#[from] solana_rpc_client::nonblocking::rpc_client::Error),
}