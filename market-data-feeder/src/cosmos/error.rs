use cosmrs::ErrorReport;
use prost::{DecodeError, EncodeError};
use thiserror::Error;

/// The various error that can be raised from [`CosmosClient`](super::client::CosmosClient).
#[derive(Error, Debug)]
pub enum Cosmos {
    #[error("Encoding error: {0}")]
    Encode(String),

    #[error("{0}")]
    WalletError(#[from] Wallet),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("Decoding  error: {0}")]
    Decode(String),

    #[error("Sign error: {0}")]
    Sign(String),

    #[error("gRPC transport error: {0}")]
    GrpcTransport(#[from] tonic::transport::Error),

    #[error("gRPC response error: {0}")]
    GrpcResponse(#[from] tonic::Status),

    #[error("LCD error: {0}")]
    Lcd(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Base account not found: {0}")]
    AccountNotFound(String),

    #[error("{0}")]
    TxBuildError(#[from] TxBuild),

    #[error("{0}")]
    ErrorReport(#[from] ErrorReport),

    #[error("{0}")]
    TendermintRpc(#[from] cosmrs::rpc::Error),
}

impl From<EncodeError> for Cosmos {
    fn from(e: EncodeError) -> Self {
        Cosmos::Encode(e.to_string())
    }
}

impl From<DecodeError> for Cosmos {
    fn from(e: DecodeError) -> Self {
        Cosmos::Decode(e.to_string())
    }
}

impl From<std::io::Error> for Cosmos {
    fn from(e: std::io::Error) -> Self {
        Cosmos::Io(e.to_string())
    }
}

#[derive(Error, Debug)]
pub enum Wallet {
    #[error("sign error: {0}")]
    Sign(String),

    #[error("invalid derivation path: {0}")]
    DerivationPath(String),

    #[error("private key error: {0}")]
    PrivateKey(String),

    #[error("invalid human readable path {0}")]
    Hrp(String),

    #[error("{0}")]
    ErrorReport(#[from] ErrorReport),

    #[error("{0}")]
    Bip32(#[from] cosmrs::bip32::Error),
}

/// The various error that can be raised from [`super::tx::Builder`].
#[derive(Error, Debug)]
pub enum TxBuild {
    #[error("Encoding error: {0}")]
    Encode(String),

    #[error("Missing account information")]
    NoAccountInfo,

    #[error("Missing transaction fee")]
    NoFee,

    #[error("Sign error: {0}")]
    Sign(String),

    #[error("{0}")]
    ErrorReport(#[from] ErrorReport),

    #[error("{0}")]
    WalletError(#[from] Wallet),

    #[error("{0}")]
    Tendermint(#[from] cosmrs::tendermint::Error),
}
