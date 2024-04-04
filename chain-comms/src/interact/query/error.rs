use cosmrs::tendermint::Error as TendermintError;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum ChainId {
    #[error("RPC error occurred while querying account data! Cause: {0}")]
    Rpc(#[from] tonic::Status),
    #[error("Node didn't return default node information!")]
    NoDefaultNodeInfoReturned,
    #[error("Failed to parse chain ID! Cause: {0}")]
    ParseChainId(TendermintError),
}

#[derive(Debug, ThisError)]
pub enum Syncing {
    #[error("RPC error occurred while querying syncing status! Cause: {0}")]
    Rpc(#[from] tonic::Status),
}

#[derive(Debug, ThisError)]
pub enum LatestBlock {
    #[error("RPC error occurred while querying latest block! Cause: {0}")]
    Rpc(#[from] tonic::Status),
    #[error("Node didn't return block information!")]
    NoBlockInfoReturned,
}

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
    #[error("Raw query failed! Cause: {0}")]
    RawQuery(#[from] Raw),
    #[error(
        "Failed to deserialize smart contract's query response from JSON! \
        Data: {data:?}; Cause: {error}",
        data = String::from_utf8_lossy(_0),
        error = _1,
    )]
    DeserializeResponse(Vec<u8>, serde_json_wasm::de::Error),
}
