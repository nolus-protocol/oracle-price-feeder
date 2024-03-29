use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed to set up tendermint JSON-RPC client! Cause: {0}")]
    TendermintClient(#[from] cosmrs::rpc::Error),
    #[error("gRPC URI's scheme is not set!")]
    GrpcUriNoSchemeSet,
    #[error("gRPC URI's host is not set!")]
    GrpcUriNoHostSet,
    #[error("gRPC URI's port is not set!")]
    GrpcUriNoPortSet,
    #[error("Failed to parse provided URI! Cause: {0}")]
    InvalidUri(#[from] tonic::codegen::http::uri::InvalidUri),
    #[error("Failed to connect to node's gRPC interface! Cause: {0}")]
    Connect(#[from] tonic::transport::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
