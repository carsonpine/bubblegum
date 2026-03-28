use crate::idl::{Idl, IdlField, IdlInstruction, IdlType};
use borsh::BorshDeserialize;
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta, EncodedTransaction, UiMessage, UiRawMessage,
};
use std::collections::HashMap;
use std::str::FromStr;
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
        let ui_tx = match &tx.transaction.transaction {
            EncodedTransaction::Json(ui_tx) => ui_tx,
            _ => return Err(DecodeError::UnsupportedTransaction),
        };
        let signature = ui_tx
            .signatures
            .first()
            .cloned()
            .ok_or(DecodeError::MissingSignature)?;
        let raw_msg = match &ui_tx.message {
            UiMessage::Raw(raw) => raw,
            _ => return Err(DecodeError::UnsupportedTransaction),
        };

        let slot = tx.slot;
        let timestamp = tx.block_time.unwrap_or(0);

        let mut decoded = Vec::new();

        for (ix_idx, ix) in raw_msg.instructions.iter().enumerate() {
            let ix_program_id_str = raw_msg
                .account_keys
                .get(ix.program_id_index as usize)
                .ok_or(DecodeError::InvalidAccountIndex)?;
            let ix_program_id = Pubkey::from_str(ix_program_id_str)
                .map_err(|_| DecodeError::UnsupportedTransaction)?;
            if &ix_program_id != program_id {
                continue;
            }

            let ix_data = bs58::decode(&ix.data)
                .into_vec()
                .map_err(|_| DecodeError::UnsupportedTransaction)?;
            if ix_data.len() < 8 {
                return Err(DecodeError::InvalidInstructionData);
            }
            let discriminator = &ix_data[..8];
            let instruction = self
                .discriminator_map
                .get(discriminator)
                .ok_or(DecodeError::UnknownDiscriminator)?;

            let (signer, accounts) =
                self.resolve_accounts(raw_msg, &instruction.accounts, ix_idx)?;

            let args = if ix_data.len() > 8 {
                let mut data_slice = &ix_data[8..];
                self.decode_args(&mut data_slice, &instruction.args)?
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

    fn flatten_account_items<'a>(
        items: &'a [crate::idl::IdlAccountItem],
        out: &mut Vec<&'a crate::idl::IdlAccountItemDetailed>,
    ) {
        for item in items {
            match item {
                crate::idl::IdlAccountItem::Account(acct) => out.push(acct),
                crate::idl::IdlAccountItem::Group(group) => {
                    Self::flatten_account_items(&group.accounts, out)
                }
            }
        }
    }

    fn resolve_accounts(
        &self,
        message: &UiRawMessage,
        account_items: &[crate::idl::IdlAccountItem],
        ix_index: usize,
    ) -> Result<(String, Vec<DecodedAccount>), DecodeError> {
        let mut signer = String::new();
        let mut accounts = Vec::new();

        let ix = &message.instructions[ix_index];
        let mut flat = Vec::new();
        Self::flatten_account_items(account_items, &mut flat);

        for (i, acct) in flat.iter().enumerate() {
            let key_index = *ix.accounts.get(i).ok_or(DecodeError::InvalidAccountIndex)? as usize;
            let pubkey = message
                .account_keys
                .get(key_index)
                .ok_or(DecodeError::InvalidAccountIndex)?
                .clone();

            if acct.signer && signer.is_empty() {
                signer = pubkey.clone();
            }

            accounts.push(DecodedAccount {
                pubkey,
                is_signer: acct.signer,
                is_writable: acct.writable,
            });
        }

        Ok((signer, accounts))
    }

    fn decode_args(&self, data: &mut &[u8], fields: &[IdlField]) -> Result<Value, DecodeError> {
        let mut obj = serde_json::Map::new();

        for field in fields {
            let value = self.decode_field(data, &field.ty)?;
            obj.insert(field.name.clone(), value);
        }

        Ok(Value::Object(obj))
    }

    fn decode_field(&self, data: &mut &[u8], ty: &IdlType) -> Result<Value, DecodeError> {
        match ty {
            IdlType::Simple(s) => match s.as_str() {
                "u8" => Ok(Value::Number(u8::deserialize(data)?.into())),
                "u16" => Ok(Value::Number(u16::deserialize(data)?.into())),
                "u32" => Ok(Value::Number(u32::deserialize(data)?.into())),
                "u64" => Ok(Value::Number(u64::deserialize(data)?.into())),
                "i8" => Ok(Value::Number(i8::deserialize(data)?.into())),
                "i16" => Ok(Value::Number(i16::deserialize(data)?.into())),
                "i32" => Ok(Value::Number(i32::deserialize(data)?.into())),
                "i64" => Ok(Value::Number(i64::deserialize(data)?.into())),
                "bool" => Ok(Value::Bool(bool::deserialize(data)?)),
                "string" => {
                    let s = String::deserialize(data)?;
                    Ok(Value::String(s))
                }
                "publicKey" => {
                    let bytes: [u8; 32] = <[u8; 32]>::deserialize(data)?;
                    let pubkey = Pubkey::new_from_array(bytes);
                    Ok(Value::String(pubkey.to_string()))
                }
                _ => Ok(Value::Null),
            },
            IdlType::Array { array: (inner, size) } => {
                let mut arr = Vec::new();
                for _ in 0..*size {
                    let v = self.decode_field(data, inner)?;
                    arr.push(v);
                }
                Ok(Value::Array(arr))
            }
            IdlType::Option { option: inner } => {
                let tag = u8::deserialize(data)?;
                if tag == 0 {
                    Ok(Value::Null)
                } else {
                    self.decode_field(data, inner)
                }
            }
            IdlType::Defined { defined } => {
                let name = defined.name();
                if let Some(types) = &self.idl.types {
                    for ty_def in types {
                        if ty_def.name == name {
                            match &ty_def.ty {
                                crate::idl::IdlTypeDefKind::Struct { fields } => {
                                    let mut obj = serde_json::Map::new();
                                    for f in fields {
                                        let val = self.decode_field(data, &f.ty)?;
                                        obj.insert(f.name.clone(), val);
                                    }
                                    return Ok(Value::Object(obj));
                                }
                                crate::idl::IdlTypeDefKind::Enum { variants } => {
                                    let variant_idx = u8::deserialize(data)?;
                                    if let Some(variant) = variants.get(variant_idx as usize) {
                                        let mut obj = serde_json::Map::new();
                                        if let Some(fields) = &variant.fields {
                                            for f in fields {
                                                let val = self.decode_field(data, &f.ty)?;
                                                obj.insert(f.name.clone(), val);
                                            }
                                        }
                                        obj.insert(
                                            "__variant".to_string(),
                                            Value::String(variant.name.clone()),
                                        );
                                        return Ok(Value::Object(obj));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Value::Null)
            }
            IdlType::Vec { vec: inner } => {
                let len = u32::deserialize(data)?;
                let mut arr = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let v = self.decode_field(data, inner)?;
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
    #[error("unsupported transaction format")]
    UnsupportedTransaction,
    #[error("transaction has no signature")]
    MissingSignature,
    #[error("account index out of bounds")]
    InvalidAccountIndex,
    #[error("instruction data too short for discriminator")]
    InvalidInstructionData,
}
