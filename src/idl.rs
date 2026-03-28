use flate2::read::ZlibDecoder;
use serde::Deserialize;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::client_error::Error as RpcClientError;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
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
    Group(IdlAccountGroup),
    Account(IdlAccountItemDetailed),
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlAccountGroup {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdlAccountItemDetailed {
    pub name: String,
    #[serde(default, alias = "isMut")]
    pub writable: bool,
    #[serde(default, alias = "isSigner")]
    pub signer: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub address: Option<String>,
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
    Struct { fields: Vec<IdlField> },
    Enum { variants: Vec<IdlEnumVariant> },
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
    Option { option: Box<IdlType> },
    Vec { vec: Box<IdlType> },
    Array { array: (Box<IdlType>, usize) },
    Defined { defined: DefinedTypeRef },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DefinedTypeRef {
    Object { name: String },
    Simple(String),
}

impl DefinedTypeRef {
    pub fn name(&self) -> &str {
        match self {
            DefinedTypeRef::Object { name } => name,
            DefinedTypeRef::Simple(s) => s,
        }
    }
}

impl Idl {
    pub fn from_file(path: &str) -> Result<Self, IdlError> {
        let content = fs::read_to_string(path)?;
        let idl: Idl = serde_json::from_str(&content)?;
        Ok(idl)
    }

    pub async fn from_account(client: &RpcClient, program_id: &Pubkey) -> Result<Self, IdlError> {
        let (base, _) = Pubkey::find_program_address(&[], program_id);
        let idl_address = Pubkey::create_with_seed(&base, "anchor:idl", program_id)
            .map_err(|_| IdlError::InvalidAddress)?;

        let data = client.get_account_data(&idl_address).await?;

        let header_len = 8 + 32;
        if data.len() < header_len + 4 {
            return Err(IdlError::InvalidAccountData);
        }
        let data_len =
            u32::from_le_bytes(data[header_len..header_len + 4].try_into().unwrap()) as usize;
        let data_end = header_len + 4 + data_len;
        if data_end > data.len() {
            return Err(IdlError::InvalidAccountData);
        }
        let compressed = &data[header_len + 4..data_end];

        let mut decoder = ZlibDecoder::new(compressed);
        let mut json = String::new();
        decoder
            .read_to_string(&mut json)
            .map_err(|_| IdlError::InvalidAccountData)?;

        let idl: Idl = serde_json::from_str(&json)?;
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
    Rpc(#[from] RpcClientError),
    #[error("could not derive IDL account address")]
    InvalidAddress,
    #[error("IDL account data is malformed")]
    InvalidAccountData,
}
