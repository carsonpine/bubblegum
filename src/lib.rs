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
pub use decoder::{DecodedInstruction, DecodedAccount};
pub use idl::{Idl, ParsedIdl, compute_discriminator};
pub use db::{PostgresDb, ClickhouseDb};
pub use rpc::{HeliusRpcClient, RpcClientConfig};
pub use config::Config;
pub use indexer::Indexer;
