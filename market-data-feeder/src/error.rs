use thiserror::Error as ThisError;

use semver::SemVer;

use crate::provider::{FeedProviderError, InvalidProviderType};

#[derive(Debug, ThisError)]
pub enum Application {
    #[error("Couldn't register global default tracing dispatcher! Cause: {0}")]
    SettingGlobalLogDispatcher(#[from] tracing::dispatcher::SetGlobalDefaultError),
    #[error("Setting up RPC environment failed! Cause: {0}")]
    RpcSetup(#[from] chain_comms::rpc_setup::error::Error),
    #[error("Failed to serialize version query message as JSON! Cause: {0}")]
    SerializeVersionQueryMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Failed to query contract's version! Cause: {0}")]
    ContractVersionQuery(#[from] chain_comms::interact::error::WasmQuery),
    #[error("Contract's version is not compatible! Minimum compatible version is {minimum_compatible}, but contract's actual version is {actual}!")]
    IncompatibleContractVersion {
        minimum_compatible: SemVer,
        actual: SemVer,
    },
    #[error("Configuration error has occurred! Cause: {0}")]
    InvalidProviderType(#[from] InvalidProviderType),
    #[error("Failed to instantiate provider! Cause: {0}")]
    InstantiateProvider(#[from] FeedProviderError),
    #[error("Failed to commit price feeding transaction! Cause: {0}")]
    CommitTx(#[from] chain_comms::interact::error::GasEstimatingTxCommit),
    #[error("A worker thread has exited due to an error! Cause: {0}")]
    Worker(#[from] Worker),
}

#[derive(Debug, ThisError)]
pub enum Worker {
    #[error("Failed to serialize price feed message as JSON! Cause: {0}")]
    SerializeExecuteMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Recovery mode state watch closed!")]
    RecoveryModeWatchClosed,
}

pub type AppResult<T> = Result<T, Application>;
