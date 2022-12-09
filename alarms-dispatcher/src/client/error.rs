use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed to set up tendermint RPC client! Cause: {0}")]
    TendermintClient(#[from] cosmrs::rpc::Error),
    #[error("Failed to parse URI provided combination! Cause: {0}")]
    InvalidUri(#[from] tonic::codegen::http::uri::InvalidUri),
    #[error("Failed to connect to node's RPC interface! Cause: {0}")]
    Connect(#[from] tonic::transport::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
