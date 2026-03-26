use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

const IDL_ACCOUNT_SEED: &[u8] = b"anchor:idl";
const IDL_ACCOUNT_DISCRIMINATOR_SIZE: usize = 8;
const INSTRUCTION_DISCRIMINATOR_SIZE: usize = 8;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Idl {
    pub address: Option<String>,
    pub metadata: Option<IdlMetadata>,
    pub instructions: Vec<IdlInstruction>,
    #[serde(default)]
    pub accounts: Vec<IdlAccount>,
    #[serde(rename = "types")]
    pub type_defs: Option<Vec<IdlTypeDef>>,
    pub version: Option<String>,
    pub name: Option<String>,
    pub events: Option<Vec<IdlEvent>>,
    pub errors: Option<Vec<IdlError>>,
    pub constants: Option<Vec<IdlConstant>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub spec: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlInstruction {
    pub name: String,
    #[serde(default)]
    pub discriminator: Vec<u8>,
    #[serde(default)]
    pub args: Vec<IdlField>,
    #[serde(default)]
    pub accounts: Vec<IdlAccountItem>,
    pub docs: Option<Vec<String>>,
    pub returns: Option<IdlType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum IdlAccountItem {
    Single(IdlAccountSingle),
    Nested(IdlAccountsNested),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlAccountSingle {
    pub name: String,
    #[serde(default)]
    pub writable: bool,
    #[serde(default)]
    pub signer: bool,
    #[serde(rename = "isMut")]
    pub is_mut: Option<bool>,
    #[serde(rename = "isSigner")]
    pub is_signer: Option<bool>,
    pub optional: Option<bool>,
    pub docs: Option<Vec<String>>,
    pub address: Option<String>,
    pub pda: Option<serde_json::Value>,
    pub relations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlAccountsNested {
    pub name: String,
    pub accounts: Vec<IdlAccountItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: IdlType,
    pub docs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum IdlType {
    Primitive(String),
    Complex(IdlTypeComplex),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlTypeComplex {
    pub vec: Option<Box<IdlType>>,
    pub option: Option<Box<IdlType>>,
    pub array: Option<(Box<IdlType>, usize)>,
    pub defined: Option<IdlTypeDefined>,
    pub coption: Option<Box<IdlType>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum IdlTypeDefined {
    Simple(String),
    WithGenerics {
        name: String,
        generics: Option<Vec<serde_json::Value>>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlAccount {
    pub name: String,
    #[serde(default)]
    pub discriminator: Vec<u8>,
    #[serde(rename = "type")]
    pub account_type: Option<IdlAccountType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlAccountType {
    pub kind: String,
    pub fields: Option<Vec<IdlField>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlTypeDef {
    pub name: String,
    #[serde(rename = "type")]
    pub type_def: IdlTypeDefValue,
    pub docs: Option<Vec<String>>,
    #[serde(default)]
    pub serialization: Option<String>,
    #[serde(default)]
    pub repr: Option<serde_json::Value>,
    #[serde(default)]
    pub generics: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlTypeDefValue {
    pub kind: String,
    pub fields: Option<Vec<IdlField>>,
    pub variants: Option<Vec<IdlEnumVariant>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlEnumVariant {
    pub name: String,
    pub fields: Option<Vec<IdlField>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlEvent {
    pub name: String,
    #[serde(default)]
    pub discriminator: Vec<u8>,
    pub fields: Option<Vec<IdlField>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlError {
    pub code: u32,
    pub name: String,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdlConstant {
    pub name: String,
    #[serde(rename = "type")]
    pub constant_type: IdlType,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ParsedIdl {
    pub raw: Idl,
    pub discriminator_map: HashMap<[u8; INSTRUCTION_DISCRIMINATOR_SIZE], IdlInstruction>,
    pub program_name: String,
}

impl ParsedIdl {
    pub fn from_idl(idl: Idl) -> Result<Self> {
        let program_name = idl
            .name
            .clone()
            .or_else(|| idl.metadata.as_ref().and_then(|m| m.name.clone()))
            .unwrap_or_else(|| "unknown_program".to_string());

        let mut discriminator_map: HashMap<[u8; 8], IdlInstruction> = HashMap::new();

        for instruction in &idl.instructions {
            let disc = if instruction.discriminator.len() == INSTRUCTION_DISCRIMINATOR_SIZE {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&instruction.discriminator);
                arr
            } else {
                compute_discriminator(&instruction.name)
            };

            discriminator_map.insert(disc, instruction.clone());
        }

        Ok(ParsedIdl {
            raw: idl,
            discriminator_map,
            program_name,
        })
    }

    pub fn find_instruction(&self, data: &[u8]) -> Option<&IdlInstruction> {
        if data.len() < INSTRUCTION_DISCRIMINATOR_SIZE {
            return None;
        }
        let mut disc = [0u8; 8];
        disc.copy_from_slice(&data[..8]);
        self.discriminator_map.get(&disc)
    }
}

impl Idl {
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read IDL file at path: {}", path))?;

        let idl: Idl = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse IDL JSON from file: {}", path))?;

        if idl.instructions.is_empty() {
            return Err(anyhow!(
                "IDL at '{}' contains no instructions — likely malformed",
                path
            ));
        }

        Ok(idl)
    }

    pub async fn from_account(rpc_client: &RpcClient, program_id: &Pubkey) -> Result<Self> {
        let idl_address = derive_idl_address(program_id)?;

        tracing::debug!("Fetching on-chain IDL from account: {}", idl_address);

        let account_data = rpc_client
            .get_account_data(&idl_address)
            .await
            .with_context(|| {
                format!(
                    "Failed to fetch IDL account {} for program {}",
                    idl_address, program_id
                )
            })?;

        if account_data.len() <= IDL_ACCOUNT_DISCRIMINATOR_SIZE + 4 {
            return Err(anyhow!(
                "IDL account data is too short ({} bytes) to be valid",
                account_data.len()
            ));
        }

        let data_without_discriminator = &account_data[IDL_ACCOUNT_DISCRIMINATOR_SIZE..];

        let data_len = u32::from_le_bytes(
            data_without_discriminator[..4]
                .try_into()
                .map_err(|_| anyhow!("Failed to read IDL data length prefix"))?,
        ) as usize;

        let compressed_data = &data_without_discriminator[4..4 + data_len];

        let decompressed = decompress_idl_data(compressed_data)
            .context("Failed to decompress on-chain IDL data")?;

        let idl: Idl = serde_json::from_slice(&decompressed)
            .context("Failed to parse decompressed on-chain IDL JSON")?;

        if idl.instructions.is_empty() {
            return Err(anyhow!(
                "On-chain IDL for program {} contains no instructions",
                program_id
            ));
        }

        tracing::info!(
            "Loaded on-chain IDL for program {} with {} instructions",
            program_id,
            idl.instructions.len()
        );

        Ok(idl)
    }
}

fn derive_idl_address(program_id: &Pubkey) -> Result<Pubkey> {
    let program_signer = Pubkey::find_program_address(&[], program_id).0;
    let (idl_address, _bump) =
        Pubkey::find_program_address(&[IDL_ACCOUNT_SEED, program_signer.as_ref()], program_id);
    Ok(idl_address)
}

fn decompress_idl_data(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .context("zlib decompression of IDL data failed")?;
    Ok(decompressed)
}

pub fn compute_discriminator(instruction_name: &str) -> [u8; 8] {
    let preimage = format!("global:{}", instruction_name);
    let hash = solana_sdk::hash::hash(preimage.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash.to_bytes()[..8]);
    disc
}

pub async fn load_idl(
    idl_path: &Option<String>,
    rpc_client: &RpcClient,
    program_id: &Pubkey,
) -> Result<ParsedIdl> {
    let idl = match idl_path {
        Some(path) if path == "account" => {
            tracing::info!(
                "Loading IDL from on-chain account for program {}",
                program_id
            );
            Idl::from_account(rpc_client, program_id).await?
        }
        Some(path) => {
            tracing::info!("Loading IDL from file: {}", path);
            Idl::from_file(path)?
        }
        None => {
            tracing::info!(
                "No IDL path specified, attempting on-chain fetch for program {}",
                program_id
            );
            Idl::from_account(rpc_client, program_id).await?
        }
    };

    let parsed = ParsedIdl::from_idl(idl)?;

    tracing::info!(
        "IDL loaded: program='{}' instructions={}",
        parsed.program_name,
        parsed.discriminator_map.len()
    );

    Ok(parsed)
}
