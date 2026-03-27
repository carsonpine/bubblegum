use crate::idl::{Idl, IdlInstruction, IdlField, IdlType};
use borsh::BorshDeserialize;
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use std::collections::HashMap;
use thiserror::Error;

pub struct Decoder {
    discriminator_map: HashMap<Vec<u8>, IdlInstruction>,
    idl: Idl,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DecodedInstruction {
    pub instruction_name: String,
    pub program_id: String,
    pub signer: String,
    pub args: Value,
    pub accounts: Vec<DecodedAccount>,
    pub slot: u64,
    pub signature: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DecodedAccount {
    pub pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
}

impl Decoder {
    pub fn new(idl: Idl) -> Self {
        let discriminator_map = idl.build_discriminator_map();
        Self {
            discriminator_map,
            idl,
        }
    }

    pub fn decode_transaction(
        &self,
        tx: &EncodedConfirmedTransactionWithStatusMeta,
        program_id: &Pubkey,
    ) -> Result<Vec<DecodedInstruction>, DecodeError> {
        let mut decoded = Vec::new();
        let signature = tx.transaction.signatures[0].to_string();
        let slot = tx.slot;
        let timestamp = tx.block_time.unwrap_or(0);

        let message = &tx.transaction.message;
        let account_keys = message.account_keys();

        for (ix_idx, ix) in message.instructions().iter().enumerate() {
            let ix_program_id = &account_keys[ix.program_id_index as usize];
            if ix_program_id != program_id {
                continue;
            }

            let discriminator = &ix.data[..std::cmp::min(8, ix.data.len())];
            let instruction = self.discriminator_map.get(discriminator)
                .ok_or(DecodeError::UnknownDiscriminator)?;

            let (signer, accounts) = self.resolve_accounts(&message, &instruction.accounts, ix_idx);

            let args = if ix.data.len() > 8 {
                self.decode_args(&ix.data[8..], &instruction.args)?
            } else {
                json!({})
            };

            decoded.push(DecodedInstruction {
                instruction_name: instruction.name.clone(),
                program_id: ix_program_id.to_string(),
                signer,
                args,
                accounts,
                slot,
                signature: signature.clone(),
                timestamp,
            });
        }

        Ok(decoded)
    }

    fn resolve_accounts(
        &self,
        message: &solana_sdk::message::Message,
        account_items: &[crate::idl::IdlAccountItem],
        ix_index: usize,
    ) -> (String, Vec<DecodedAccount>) {
        let mut signer = String::new();
        let mut accounts = Vec::new();

        for item in account_items {
            let (name, is_mut, is_signer) = match item {
                crate::idl::IdlAccountItem::Single(name) => (name, false, false),
                crate::idl::IdlAccountItem::Detailed(d) => (&d.name, d.is_mut, d.is_signer),
            };

            let key = &message.account_keys[message.instructions()[ix_index].accounts[0] as usize];
            let pubkey = key.to_string();

            if is_signer && signer.is_empty() {
                signer = pubkey.clone();
            }

            accounts.push(DecodedAccount {
                pubkey,
                is_signer,
                is_writable: is_mut,
            });
        }

        (signer, accounts)
    }

    fn decode_args(&self, data: &[u8], fields: &[IdlField]) -> Result<Value, DecodeError> {
        let mut cursor = std::io::Cursor::new(data);
        let mut obj = serde_json::Map::new();

        for field in fields {
            let value = self.decode_field(&mut cursor, &field.ty)?;
            obj.insert(field.name.clone(), value);
        }

        Ok(Value::Object(obj))
    }

    fn decode_field(&self, cursor: &mut std::io::Cursor<&[u8]>, ty: &IdlType) -> Result<Value, DecodeError> {
        match ty {
            IdlType::Simple(s) => match s.as_str() {
                "u8" => Ok(Value::Number(u8::deserialize(cursor)?.into())),
                "u16" => Ok(Value::Number(u16::deserialize(cursor)?.into())),
                "u32" => Ok(Value::Number(u32::deserialize(cursor)?.into())),
                "u64" => Ok(Value::Number(u64::deserialize(cursor)?.into())),
                "i8" => Ok(Value::Number(i8::deserialize(cursor)?.into())),
                "i16" => Ok(Value::Number(i16::deserialize(cursor)?.into())),
                "i32" => Ok(Value::Number(i32::deserialize(cursor)?.into())),
                "i64" => Ok(Value::Number(i64::deserialize(cursor)?.into())),
                "bool" => Ok(Value::Bool(bool::deserialize(cursor)?)),
                "string" => {
                    let s = String::deserialize(cursor)?;
                    Ok(Value::String(s))
                }
                "publicKey" => {
                    let bytes: [u8; 32] = <[u8; 32]>::deserialize(cursor)?;
                    let pubkey = Pubkey::new_from_array(bytes);
                    Ok(Value::String(pubkey.to_string()))
                }
                _ => Ok(Value::Null),
            },
            IdlType::Array { array, size } => {
                let mut arr = Vec::new();
                for _ in 0..*size {
                    for inner in array {
                        let v = self.decode_field(cursor, inner)?;
                        arr.push(v);
                    }
                }
                Ok(Value::Array(arr))
            }
            IdlType::Option(inner) => {
                let tag = u8::deserialize(cursor)?;
                if tag == 0 {
                    Ok(Value::Null)
                } else {
                    self.decode_field(cursor, inner)
                }
            }
            IdlType::Defined(name) => {
                if let Some(types) = &self.idl.types {
                    for ty_def in types {
                        if ty_def.name == *name {
                            match &ty_def.ty {
                                crate::idl::IdlTypeDefKind::Struct { fields } => {
                                    let mut obj = serde_json::Map::new();
                                    for f in fields {
                                        let val = self.decode_field(cursor, &f.ty)?;
                                        obj.insert(f.name.clone(), val);
                                    }
                                    return Ok(Value::Object(obj));
                                }
                                crate::idl::IdlTypeDefKind::Enum { variants } => {
                                    let variant_idx = u8::deserialize(cursor)?;
                                    if let Some(variant) = variants.get(variant_idx as usize) {
                                        let mut obj = serde_json::Map::new();
                                        if let Some(fields) = &variant.fields {
                                            for f in fields {
                                                let val = self.decode_field(cursor, &f.ty)?;
                                                obj.insert(f.name.clone(), val);
                                            }
                                        }
                                        obj.insert("__variant".to_string(), Value::String(variant.name.clone()));
                                        return Ok(Value::Object(obj));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Value::Null)
            }
            IdlType::Vec(inner) => {
                let len = u32::deserialize(cursor)?;
                let mut arr = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let v = self.decode_field(cursor, inner)?;
                    arr.push(v);
                }
                Ok(Value::Array(arr))
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unknown instruction discriminator")]
    UnknownDiscriminator,
    #[error("borsh decode error: {0}")]
    Borsh(#[from] std::io::Error),
}