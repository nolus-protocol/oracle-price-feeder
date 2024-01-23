use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed signing execution message! Cause: {0}")]
    Signing(#[from] crate::signer::error::Error),
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
}
