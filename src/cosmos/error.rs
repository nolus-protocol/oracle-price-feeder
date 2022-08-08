use cosmrs::ErrorReport;
use prost::{DecodeError, EncodeError};
use thiserror::Error;

/// The various error that can be raised from [`super::client::CosmosClient`].
#[derive(Error, Debug)]
pub enum CosmosError {
    #[error("Encoding error: {0}")]
    Encode(String),

    #[error("{0}")]
    WalletError(#[from] WalletError),

    #[error("{0}")]
    Json(#[from] serde_json::Error),

    #[error("Decoding  error: {0}")]
    Decode(String),

    #[error("Sign error: {0}")]
    Sign(String),

    #[error("gRPC error: {0}")]
    Grpc(String),

    #[error("LCD error: {0}")]
    Lcd(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Base account not found: {0}")]
    AccountNotFound(String),

    #[error("{0}")]
    TxBuildError(#[from] TxBuildError),

    #[error("{0}")]
    ErrorReport(#[from] ErrorReport),

    #[error("{0}")]
    TendermintRpc(#[from] cosmrs::rpc::Error),
}

impl From<EncodeError> for CosmosError {
    fn from(e: EncodeError) -> Self {
        CosmosError::Encode(e.to_string())
    }
}

impl From<DecodeError> for CosmosError {
    fn from(e: DecodeError) -> Self {
        CosmosError::Decode(e.to_string())
    }
}

impl From<tonic::transport::Error> for CosmosError {
    fn from(e: tonic::transport::Error) -> Self {
        CosmosError::Grpc(e.to_string())
    }
}

impl From<std::io::Error> for CosmosError {
    fn from(e: std::io::Error) -> Self {
        CosmosError::Io(e.to_string())
    }
}

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("sign error: {0}")]
    Sign(String),

    #[error("mnemonic error: {0}")]
    Mnemonic(String),

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

/// The various error that can be raised from [`super::tx::TxBuilder`].
#[derive(Error, Debug)]
pub enum TxBuildError {
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
    WalletError(#[from] WalletError),

    #[error("{0}")]
    Tendermint(#[from] cosmrs::tendermint::Error),
}
