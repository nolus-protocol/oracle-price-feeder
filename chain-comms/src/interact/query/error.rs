use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum AccountData {
    #[error("RPC error occurred while querying account data! Cause: {0}")]
    Rpc(#[from] tonic::Status),
    #[error("Node failed to provide information about requested address!")]
    NoAccountData,
    #[error("Failed to deserialize account data from protobuf! Cause: {0}")]
    DeserializeAccountData(#[from] prost::DecodeError),
}

#[derive(Debug, ThisError)]
pub enum Raw {
    #[error("Connection failure occurred! Cause: {0}")]
    Transport(#[from] tonic::transport::Error),
    #[error("RPC responded with a failure! Cause: {0}")]
    Response(#[from] tonic::Status),
}

#[derive(Debug, ThisError)]
pub enum Wasm {
    #[error("{0}")]
    RawQuery(#[from] Raw),
    #[error("Failed to deserialize smart contract's query response from JSON! Cause: {0}")]
    DeserializeResponse(#[from] serde_json_wasm::de::Error),
}
