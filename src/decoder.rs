use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiMessage,
    UiTransactionStatusMeta,
};
use std::sync::Arc;

use crate::idl::{IdlAccountItem, IdlField, IdlType, IdlTypeComplex, IdlTypeDefined, ParsedIdl};

const INSTRUCTION_DISCRIMINATOR_SIZE: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedInstruction {
    pub instruction_name: String,
    pub program_id: String,
    pub signer: String,
    pub args: serde_json::Value,
    pub accounts: Vec<DecodedAccount>,
    pub slot: u64,
    pub signature: String,
    pub timestamp: i64,
    pub raw_accounts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedAccount {
    pub name: String,
    pub pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
}

pub struct TransactionDecoder {
    idl: Arc<ParsedIdl>,
    program_id: String,
}

impl TransactionDecoder {
    pub fn new(idl: Arc<ParsedIdl>, program_id: String) -> Self {
        TransactionDecoder { idl, program_id }
    }

    pub fn decode_transaction(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
    ) -> Result<Vec<DecodedInstruction>> {
        let slot = tx.slot;
        let timestamp = tx.block_time.unwrap_or(0);

        let (message, signature, _meta) = extract_transaction_parts(tx)?;

        let account_keys = extract_account_keys(&message)?;
        let signer = account_keys.first().cloned().unwrap_or_default();

        let instructions = extract_instructions(&message)?;

        let mut decoded = Vec::new();

        for raw_ix in instructions {
            let program_account_index = raw_ix.program_id_index as usize;
            if program_account_index >= account_keys.len() {
                continue;
            }

            let ix_program_id = &account_keys[program_account_index];
            if ix_program_id != &self.program_id {
                continue;
            }

            let data = decode_instruction_data(&raw_ix.data)?;

            if data.len() < INSTRUCTION_DISCRIMINATOR_SIZE {
                tracing::debug!(
                    signature = %signature,
                    "Instruction data too short ({} bytes), skipping",
                    data.len()
                );
                continue;
            }

            let idl_instruction = match self.idl.find_instruction(&data) {
                Some(ix) => ix,
                None => {
                    let disc: [u8; 8] = data[..8].try_into().unwrap_or([0u8; 8]);
                    tracing::debug!(
                        signature = %signature,
                        discriminator = ?disc,
                        "Unknown discriminator, instruction not in IDL"
                    );
                    continue;
                }
            };

            let args_data = &data[INSTRUCTION_DISCRIMINATOR_SIZE..];
            let args =
                decode_args(&idl_instruction.args, args_data, &self.idl).unwrap_or_else(|e| {
                    tracing::warn!(
                        signature = %signature,
                        instruction = %idl_instruction.name,
                        error = %e,
                        "Failed to decode instruction args, storing raw hex"
                    );
                    serde_json::json!({ "raw_hex": hex::encode(args_data) })
                });

            let mut decoded_accounts: Vec<DecodedAccount> = Vec::new();
            let raw_accounts: Vec<String> = raw_ix
                .accounts
                .iter()
                .filter_map(|&idx| account_keys.get(idx as usize).cloned())
                .collect();

            for (i, idl_acc_item) in idl_instruction.accounts.iter().enumerate() {
                let pubkey = raw_ix
                    .accounts
                    .get(i)
                    .and_then(|&idx| account_keys.get(idx as usize))
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                match idl_acc_item {
                    IdlAccountItem::Single(single) => {
                        let is_signer = single.signer || single.is_signer.unwrap_or(false);
                        let is_writable = single.writable || single.is_mut.unwrap_or(false);
                        decoded_accounts.push(DecodedAccount {
                            name: single.name.clone(),
                            pubkey,
                            is_signer,
                            is_writable,
                        });
                    }
                    IdlAccountItem::Nested(nested) => {
                        decoded_accounts.push(DecodedAccount {
                            name: nested.name.clone(),
                            pubkey,
                            is_signer: false,
                            is_writable: false,
                        });
                    }
                }
            }

            let effective_signer = decoded_accounts
                .iter()
                .find(|a| a.is_signer)
                .map(|a| a.pubkey.clone())
                .unwrap_or_else(|| signer.clone());

            decoded.push(DecodedInstruction {
                instruction_name: idl_instruction.name.clone(),
                program_id: self.program_id.clone(),
                signer: effective_signer,
                args,
                accounts: decoded_accounts,
                slot,
                signature: signature.clone(),
                timestamp,
                raw_accounts,
            });
        }

        Ok(decoded)
    }
}

struct RawInstruction {
    program_id_index: u8,
    accounts: Vec<u8>,
    data: String,
}

fn extract_transaction_parts(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<(UiMessage, String, Option<&UiTransactionStatusMeta>)> {
    let (message, signature) = match &tx.transaction.transaction {
        EncodedTransaction::Json(ui_tx) => {
            let sig = ui_tx
                .signatures
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            (ui_tx.message.clone(), sig)
        }
        EncodedTransaction::Binary(_data, _encoding) => {
            return Err(anyhow!(
                "Binary-encoded transactions are not supported in this decoder path"
            ));
        }
        EncodedTransaction::Accounts(ui_accounts_tx) => {
            let sig = ui_accounts_tx
                .signatures
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            return Err(anyhow!(
                "Accounts-only encoded transaction {} cannot be fully decoded without message data",
                sig
            ));
        }
        EncodedTransaction::LegacyBinary(_) => {
            return Err(anyhow!("Legacy binary transaction format not supported"));
        }
    };

    let meta = tx.transaction.meta.as_ref();
    Ok((message, signature, meta))
}

fn extract_account_keys(message: &UiMessage) -> Result<Vec<String>> {
    match message {
        UiMessage::Parsed(parsed) => Ok(parsed
            .account_keys
            .iter()
            .map(|k| k.pubkey.clone())
            .collect()),
        UiMessage::Raw(raw) => Ok(raw.account_keys.clone()),
    }
}

fn extract_instructions(message: &UiMessage) -> Result<Vec<RawInstruction>> {
    match message {
        UiMessage::Raw(raw) => {
            let instructions = raw
                .instructions
                .iter()
                .map(|ix| RawInstruction {
                    program_id_index: ix.program_id_index,
                    accounts: ix.accounts.clone(),
                    data: ix.data.clone(),
                })
                .collect();
            Ok(instructions)
        }
        UiMessage::Parsed(parsed) => {
            let mut instructions = Vec::new();
            for ix in &parsed.instructions {
                match ix {
                    solana_transaction_status::UiInstruction::Compiled(compiled) => {
                        instructions.push(RawInstruction {
                            program_id_index: compiled.program_id_index,
                            accounts: compiled.accounts.clone(),
                            data: compiled.data.clone(),
                        });
                    }
                    solana_transaction_status::UiInstruction::Parsed(_) => {
                        // Parsed instructions don't carry raw bytes; skip them
                    }
                }
            }
            Ok(instructions)
        }
    }
}

fn decode_instruction_data(data_b58: &str) -> Result<Vec<u8>> {
    bs58::decode(data_b58)
        .into_vec()
        .with_context(|| format!("Failed to base58-decode instruction data: '{}'", data_b58))
}

pub fn decode_args(fields: &[IdlField], data: &[u8], idl: &ParsedIdl) -> Result<serde_json::Value> {
    let mut reader = BorshReader::new(data);
    let mut map = serde_json::Map::new();

    for field in fields {
        let value = decode_type(&field.field_type, &mut reader, idl)
            .with_context(|| format!("Failed to decode field '{}'", field.name))?;
        map.insert(field.name.clone(), value);
    }

    Ok(serde_json::Value::Object(map))
}

fn decode_type(
    idl_type: &IdlType,
    reader: &mut BorshReader,
    idl: &ParsedIdl,
) -> Result<serde_json::Value> {
    match idl_type {
        IdlType::Primitive(name) => decode_primitive(name, reader),
        IdlType::Complex(complex) => decode_complex(complex, reader, idl),
    }
}

fn decode_primitive(name: &str, reader: &mut BorshReader) -> Result<serde_json::Value> {
    match name {
        "bool" => {
            let b = reader.read_u8().context("reading bool")?;
            Ok(serde_json::Value::Bool(b != 0))
        }
        "u8" => {
            let v = reader.read_u8().context("reading u8")?;
            Ok(serde_json::Value::Number(v.into()))
        }
        "i8" => {
            let v = reader.read_i8().context("reading i8")?;
            Ok(serde_json::Value::Number(v.into()))
        }
        "u16" => {
            let v = reader.read_u16().context("reading u16")?;
            Ok(serde_json::Value::Number(v.into()))
        }
        "i16" => {
            let v = reader.read_i16().context("reading i16")?;
            Ok(serde_json::Value::Number(v.into()))
        }
        "u32" => {
            let v = reader.read_u32().context("reading u32")?;
            Ok(serde_json::Value::Number(v.into()))
        }
        "i32" => {
            let v = reader.read_i32().context("reading i32")?;
            Ok(serde_json::Value::Number(v.into()))
        }
        "u64" => {
            let v = reader.read_u64().context("reading u64")?;
            Ok(serde_json::json!(v.to_string()))
        }
        "i64" => {
            let v = reader.read_i64().context("reading i64")?;
            Ok(serde_json::json!(v.to_string()))
        }
        "u128" => {
            let v = reader.read_u128().context("reading u128")?;
            Ok(serde_json::json!(v.to_string()))
        }
        "i128" => {
            let v = reader.read_i128().context("reading i128")?;
            Ok(serde_json::json!(v.to_string()))
        }
        "f32" => {
            let v = reader.read_f32().context("reading f32")?;
            Ok(serde_json::json!(v))
        }
        "f64" => {
            let v = reader.read_f64().context("reading f64")?;
            Ok(serde_json::json!(v))
        }
        "string" | "String" => {
            let s = reader.read_string().context("reading String")?;
            Ok(serde_json::Value::String(s))
        }
        "publicKey" | "pubkey" | "Pubkey" => {
            let bytes = reader.read_bytes(32).context("reading Pubkey")?;
            let pubkey = bs58::encode(&bytes).into_string();
            Ok(serde_json::Value::String(pubkey))
        }
        "bytes" => {
            let len = reader.read_u32().context("reading bytes length")? as usize;
            let bytes = reader.read_bytes(len).context("reading bytes data")?;
            Ok(serde_json::json!(hex::encode(&bytes)))
        }
        unknown => {
            tracing::debug!("Unknown primitive type '{}', skipping decode", unknown);
            Err(anyhow!("Unknown primitive IDL type: '{}'", unknown))
        }
    }
}

fn decode_complex(
    complex: &IdlTypeComplex,
    reader: &mut BorshReader,
    idl: &ParsedIdl,
) -> Result<serde_json::Value> {
    if let Some(inner) = &complex.vec {
        let len = reader.read_u32().context("reading Vec length")? as usize;
        let mut arr = Vec::with_capacity(len);
        for i in 0..len {
            let val = decode_type(inner, reader, idl)
                .with_context(|| format!("decoding Vec element {}", i))?;
            arr.push(val);
        }
        return Ok(serde_json::Value::Array(arr));
    }

    if let Some(inner) = &complex.option {
        let discriminant = reader.read_u8().context("reading Option discriminant")?;
        if discriminant == 0 {
            return Ok(serde_json::Value::Null);
        } else {
            return decode_type(inner, reader, idl).context("decoding Option::Some value");
        }
    }

    if let Some((inner_type, length)) = &complex.array {
        let mut arr = Vec::with_capacity(*length);
        for i in 0..*length {
            let val = decode_type(inner_type, reader, idl)
                .with_context(|| format!("decoding fixed array element {}", i))?;
            arr.push(val);
        }
        return Ok(serde_json::Value::Array(arr));
    }

    if let Some(defined) = &complex.defined {
        let type_name = match defined {
            IdlTypeDefined::Simple(s) => s.as_str(),
            IdlTypeDefined::WithGenerics { name, .. } => name.as_str(),
        };
        return decode_defined_type(type_name, reader, idl);
    }

    if let Some(inner) = &complex.coption {
        let discriminant = reader.read_u32().context("reading COption discriminant")?;
        if discriminant == 0 {
            return Ok(serde_json::Value::Null);
        } else {
            return decode_type(inner, reader, idl).context("decoding COption::Some value");
        }
    }

    Err(anyhow!("Unrecognized complex IDL type structure"))
}

fn decode_defined_type(
    type_name: &str,
    reader: &mut BorshReader,
    idl: &ParsedIdl,
) -> Result<serde_json::Value> {
    let type_defs = match &idl.raw.type_defs {
        Some(defs) => defs,
        None => {
            return Err(anyhow!(
                "IDL has no type definitions, cannot decode '{}'",
                type_name
            ))
        }
    };

    let typedef = type_defs
        .iter()
        .find(|t| t.name == type_name)
        .ok_or_else(|| anyhow!("Type '{}' not found in IDL type definitions", type_name))?;

    match typedef.type_def.kind.as_str() {
        "struct" => {
            let fields = typedef.type_def.fields.as_deref().unwrap_or(&[]);
            let mut map = serde_json::Map::new();
            for field in fields {
                let value = decode_type(&field.field_type, reader, idl)
                    .with_context(|| format!("decoding struct field '{}'", field.name))?;
                map.insert(field.name.clone(), value);
            }
            Ok(serde_json::Value::Object(map))
        }
        "enum" => {
            let variant_index = reader.read_u8().context("reading enum variant index")? as usize;
            let variants = typedef.type_def.variants.as_deref().unwrap_or(&[]);
            let variant = variants.get(variant_index).ok_or_else(|| {
                anyhow!(
                    "Enum '{}' variant index {} out of range (total variants: {})",
                    type_name,
                    variant_index,
                    variants.len()
                )
            })?;

            if let Some(fields) = &variant.fields {
                let mut map = serde_json::Map::new();
                for (i, field) in fields.iter().enumerate() {
                    let field_type = match field {
                        crate::idl::IdlEnumField::Full(f) => &f.field_type,
                        crate::idl::IdlEnumField::Simple(type_str) => {
                            &crate::idl::IdlType::Primitive(type_str.clone())
                        }
                    };
                    let field_name = match field {
                        crate::idl::IdlEnumField::Full(f) => f.name.clone(),
                        crate::idl::IdlEnumField::Simple(_) => format!("field_{}", i),
                    };
                    let value = decode_type(field_type, reader, idl)
                        .with_context(|| format!("decoding enum variant field '{}'", field_name))?;
                    map.insert(field_name, value);
                }
                Ok(serde_json::json!({ &variant.name: map }))
            } else {
                Ok(serde_json::Value::String(variant.name.clone()))
            }
        }
        other => Err(anyhow!(
            "Unsupported type definition kind '{}' for type '{}'",
            other,
            type_name
        )),
    }
}

struct BorshReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BorshReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BorshReader { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        if self.pos + n > self.data.len() {
            return Err(anyhow!(
                "BorshReader underflow: need {} bytes at pos {}, but only {} remain",
                n,
                self.pos,
                self.remaining()
            ));
        }
        let slice = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8> {
        let b = self.read_bytes(1)?;
        Ok(b[0])
    }

    fn read_i8(&mut self) -> Result<i8> {
        Ok(self.read_u8()? as i8)
    }

    fn read_u16(&mut self) -> Result<u16> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i16(&mut self) -> Result<i16> {
        let b = self.read_bytes(2)?;
        Ok(i16::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i32(&mut self) -> Result<i32> {
        let b = self.read_bytes(4)?;
        Ok(i32::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let b = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i64(&mut self) -> Result<i64> {
        let b = self.read_bytes(8)?;
        Ok(i64::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_u128(&mut self) -> Result<u128> {
        let b = self.read_bytes(16)?;
        Ok(u128::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i128(&mut self) -> Result<i128> {
        let b = self.read_bytes(16)?;
        Ok(i128::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_f32(&mut self) -> Result<f32> {
        let b = self.read_bytes(4)?;
        Ok(f32::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_f64(&mut self) -> Result<f64> {
        let b = self.read_bytes(8)?;
        Ok(f64::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes).context("Invalid UTF-8 in borsh-encoded string")
    }
}
