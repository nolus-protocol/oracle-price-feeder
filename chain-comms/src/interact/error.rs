use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum AccountQuery {
    #[error("Connection failure occurred! Cause: {0}")]
    Connection(#[from] tonic::Status),
    #[error("Node failed to provide information about requested address!")]
    NoAccountData,
    #[error("Failed to deserialize account data from protobuf! Cause: {0}")]
    DeserializeAccountData(#[from] cosmrs::proto::prost::DecodeError),
}

#[derive(Debug, ThisError)]
pub enum WasmQuery {
    #[error("Connection failure occurred! Cause: {0}")]
    Connection(#[from] tonic::Status),
    #[error("Failed to deserialize smart contract's query response from JSON! Cause: {0}")]
    DeserializeResponse(#[from] serde_json_wasm::de::Error),
}

#[derive(Debug, ThisError)]
#[error("Failed to calculate and construct fee object! Cause: {0}")]
pub struct FeeCalculation(#[from] cosmrs::ErrorReport);

#[derive(Debug, ThisError)]
pub enum SimulateTx {
    #[error("Failed committing and signing execution message! Cause: {0}")]
    Commit(#[from] crate::build_tx::error::Error),
    #[error("Failed serializing transaction as bytes! Cause: {0}")]
    SerializeTransaction(#[from] cosmrs::ErrorReport),
    #[error("Attempt to run simulation resulted in an error! Cause: {0}")]
    SimulationRunError(#[from] tonic::Status),
    #[error("Simulation result is missing gas into!")]
    MissingSimulationGasInto,
    #[error("Simulation result's used gas exceeds gas limit! Simulation gas used: {used}.")]
    SimulationGasExceedsLimit { used: u64 },
    #[error("Failed to calculate and construct fee object! Cause: {0}")]
    FeeCalculation(#[from] FeeCalculation),
}

#[derive(Debug, ThisError)]
pub enum CommitTx {
    #[error("Failed to calculate and construct fee object! Cause: {0}")]
    FeeCalculation(#[from] FeeCalculation),
    #[error("Failed committing and signing execution message! Cause: {0}")]
    Commit(#[from] crate::build_tx::error::Error),
    #[error("Failed to broadcast committed message! Cause: {0}")]
    Broadcast(#[from] cosmrs::ErrorReport),
}

#[derive(Debug, ThisError)]
pub enum GasEstimatingTxCommit {
    #[error("Transaction simulation failed! Cause: {0}")]
    SimulationFailed(#[from] SimulateTx),
    #[error("Transaction committing and broadcasting failed! Cause: {0}")]
    CommitFailed(#[from] CommitTx),
}
