// Library root for Bubblegum Solana Indexer
// This enables testing of internal modules

pub mod api;
pub mod config;
pub mod db;
pub mod decoder;
pub mod idl;
pub mod indexer;
pub mod rpc;

// Re-export commonly used types
pub use config::Config;
pub use db::{ClickhouseDb, PostgresDb};
pub use decoder::{DecodedAccount, DecodedInstruction};
pub use idl::{compute_discriminator, Idl, ParsedIdl};
pub use indexer::Indexer;
pub use rpc::{HeliusRpcClient, RpcClientConfig};
