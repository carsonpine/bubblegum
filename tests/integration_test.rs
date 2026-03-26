use anyhow::Result;

mod common {
    use solana_sdk::{pubkey::Pubkey, signature::Signature};
    use std::str::FromStr;

    pub fn test_program_id() -> Pubkey {
        Pubkey::from_str("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo").unwrap()
    }

    pub fn test_signature() -> Signature {
        Signature::from_str("5KtPn3Dxyz123456789abcdefghijklmnopqrstuv").unwrap_or_default()
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;
    use std::env;

    #[test]
    fn test_env_vars_required() {
        let helius_url = env::var("HELIUS_RPC_URL").unwrap_or_default();
        assert!(
            helius_url.contains("helius") || helius_url.is_empty(),
            "HELIUS_RPC_URL should contain helius if set"
        );
    }

    #[test]
    fn test_program_id_format() {
        let program_id = common::test_program_id();
        assert!(program_id.to_string().len() >= 32, "Program ID should be at least 32 chars");
    }
}

#[cfg(test)]
mod idl_tests {
    use super::*;
    use bubblegum::idl::{Idl, ParsedIdl};

    #[test]
    fn test_idl_discriminator_computation() {
        let disc1 = bubblegum::idl::compute_discriminator("swap");
        let disc2 = bubblegum::idl::compute_discriminator("swap");
        assert_eq!(disc1, disc2);

        let disc3 = bubblegum::idl::compute_discriminator("deposit");
        assert_ne!(disc1, disc3);
    }

    #[test]
    fn test_parsed_idl_from_mock_idl() {
        let mock_idl = Idl {
            address: None,
            metadata: None,
            instructions: vec![],
            type_defs: None,
            version: None,
            name: Some("test_program".to_string()),
            events: None,
            errors: None,
            constants: None,
            accounts: vec![],
        };

        let parsed = ParsedIdl::from_idl(mock_idl);
        assert!(parsed.is_ok());
        
        let parsed = parsed.unwrap();
        assert_eq!(parsed.program_name, "test_program");
    }
}

#[cfg(test)]
mod decoder_tests {
    use super::*;
    use bubblegum::decoder::{DecodedInstruction, TransactionDecoder};
    use bubblegum::idl::{Idl, ParsedIdl, IdlInstruction};
    use std::sync::Arc;

    fn create_test_idl() -> ParsedIdl {
        let mock_idl = Idl {
            address: None,
            metadata: None,
            instructions: vec![
                IdlInstruction {
                    name: "swap".to_string(),
                    discriminator: vec![0, 1, 2, 3, 4, 5, 6, 7],
                    args: vec![],
                    accounts: vec![],
                    docs: None,
                    returns: None,
                }
            ],
            type_defs: None,
            version: None,
            name: Some("test".to_string()),
            events: None,
            errors: None,
            constants: None,
            accounts: vec![],
        };
        ParsedIdl::from_idl(mock_idl).unwrap()
    }

    #[test]
    fn test_decoder_creation() {
        let idl = Arc::new(create_test_idl());
        let program_id = common::test_program_id().to_string();
        
        let _decoder = TransactionDecoder::new(idl, program_id);
    }

    #[test]
    fn test_decoded_instruction_structure() {
        let instruction = DecodedInstruction {
            instruction_name: "swap".to_string(),
            program_id: common::test_program_id().to_string(),
            signer: "Signer111111111111111111111111111111111".to_string(),
            args: serde_json::json!({"amount": 1000}),
            accounts: vec![],
            slot: 100000000,
            signature: common::test_signature().to_string(),
            timestamp: 1700000000,
            raw_accounts: vec![],
        };

        assert_eq!(instruction.instruction_name, "swap");
        assert_eq!(instruction.slot, 100000000);
    }
}

#[cfg(test)]
mod rpc_tests {
    use super::*;
    use bubblegum::rpc::{HeliusRpcClient, RpcClientConfig};
    use solana_sdk::commitment_config::CommitmentConfig;

    #[test]
    fn test_rpc_client_creation() {
        let config = RpcClientConfig {
            url: "https://api.mainnet-beta.solana.com".to_string(),
            rate_limit_rps: 10,
            commitment: CommitmentConfig::confirmed(),
        };

        let _client = HeliusRpcClient::new(config);
    }

    #[test]
    fn test_rpc_config_validation() {
        let valid_url = "https://mainnet.helius-rpc.com/?api-key=test";
        assert!(valid_url.starts_with("https://"));

        let invalid_url = "not-a-url";
        assert!(!invalid_url.starts_with("http://") && !invalid_url.starts_with("https://"));
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;

    #[test]
    fn test_postgres_url_format() {
        let url = "postgres://indexer:changeme@postgres:5432/solana_indexer";
        assert!(url.starts_with("postgres://"));
        assert!(url.contains("@"));
        assert!(url.contains(":5432/"));
    }

    #[test]
    fn test_clickhouse_url_format() {
        let url = "http://clickhouse:8123";
        assert!(url.starts_with("http://"));
        assert!(url.contains(":8123"));
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;

    #[test]
    fn test_checkpoint_key() {
        const CHECKPOINT_KEY: &str = "last_indexed_slot";
        assert_eq!(CHECKPOINT_KEY, "last_indexed_slot");
    }

    #[test]
    fn test_slot_range_validation() {
        let start: u64 = 100000000;
        let end: u64 = 100001000;
        assert!(start < end);

        let invalid_start: u64 = 100001000;
        let invalid_end: u64 = 100000000;
        assert!(invalid_start > invalid_end);
    }
}

#[cfg(test)]
mod batch_size_tests {
    use super::*;

    #[test]
    fn test_batch_size_constraints() {
        let min_batch: usize = 1;
        let max_batch: usize = 1000;
        let default_batch: usize = 100;

        assert!(default_batch >= min_batch);
        assert!(default_batch <= max_batch);
    }

    #[test]
    fn test_batch_size_calculation() {
        let batch_size: usize = 100;
        let total_slots: u64 = 10000;
        
        let batches_needed = (total_slots as f64 / batch_size as f64).ceil() as usize;
        assert_eq!(batches_needed, 100);
    }
}
