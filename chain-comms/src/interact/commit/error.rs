use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum CommitTx {
    #[error("Failed signing execution message! Cause: {0}")]
    Signing(#[from] crate::signer::error::Error),
    #[error("Failed committing and signing execution message! Cause: {0}")]
    Commit(#[from] crate::build_tx::error::Error),
    #[error("Failed to serialize committed transaction! Cause: {0}")]
    Serialize(#[from] cosmrs::ErrorReport),
    #[error("Failed to broadcast committed transaction! Cause: {0}")]
    Broadcast(#[from] cosmrs::rpc::Error),
}

#[derive(Debug, ThisError)]
pub enum GasEstimatingTxCommit {
    #[error("Transaction simulation failed! Cause: {0}")]
    SimulationFailed(#[from] crate::interact::simulate::error::Error),
    #[error("Transaction committing and broadcasting failed! Cause: {0}")]
    CommitFailed(#[from] CommitTx),
}
