use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum CommitTx {
    #[error("Failed signing execution message! Cause: {0}")]
    Signing(#[from] crate::signer::error::Error),
    #[error("Failed committing and signing execution message! Cause: {0}")]
    Commit(#[from] crate::build_tx::error::Error),
    #[error("Failed to broadcast committed transaction! Cause: {0}")]
    Broadcast(#[from] tonic::Status),
    #[error("Broadcast completed but didn't no response was found! Broadcast may have failed!")]
    EmptyResponseReceived,
}

#[derive(Debug, ThisError)]
pub enum GasEstimatingTxCommit {
    #[error("Transaction simulation failed! Cause: {0}")]
    SimulationFailed(#[from] crate::interact::simulate::error::Error),
    #[error("Transaction committing and broadcasting failed! Cause: {0}")]
    CommitFailed(#[from] CommitTx),
}
