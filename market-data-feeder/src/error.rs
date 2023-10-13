use std::{error::Error as StdError, sync::Arc};

use thiserror::Error as ThisError;

use semver::Version;

use crate::provider::PriceComparisonGuardError;

#[derive(Debug, ThisError)]
pub(crate) enum Application {
    #[error("Couldn't register global default tracing dispatcher! Cause: {0}")]
    SettingGlobalLogDispatcher(#[from] tracing::dispatcher::SetGlobalDefaultError),
    #[error("Setting up RPC environment failed! Cause: {0}")]
    RpcSetup(#[from] chain_comms::rpc_setup::error::Error),
    #[error("Failed to serialize version query message as JSON! Cause: {0}")]
    SerializeVersionQueryMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Failed to query contract's version! Cause: {0}")]
    ContractVersionQuery(#[from] chain_comms::interact::error::WasmQuery),
    #[error("Oracle \"{oracle_addr}\"'s version is not compatible! Minimum compatible version is {compatible}, but contract's actual version is {actual}!")]
    IncompatibleContractVersion {
        oracle_addr: Arc<str>,
        compatible: semver::Comparator,
        actual: Version,
    },
    #[error("Unknown provider identifier! Got: {0}")]
    UnknownProviderId(Arc<str>),
    #[error("Unknown price comparison provider identifier! Got: {0}")]
    UnknownPriceComparisonProviderId(Arc<str>),
    #[error("Failed to instantiate provider! Cause: {0}")]
    InvalidProviderUrl(#[from] url::ParseError),
    #[error("Failed to commit price feeding transaction! Cause: {0}")]
    CommitTx(#[from] chain_comms::interact::error::GasEstimatingTxCommit),
    #[error("A worker thread has exited due to an error! Cause: {0}")]
    Worker(#[from] Worker),
}

#[derive(Debug, ThisError)]
pub(crate) enum Worker {
    #[error("Failed to instantiate provider with id: {0}! Cause: {1}")]
    InstantiateProvider(String, Box<dyn StdError + Send + 'static>),
    #[error("Failed to instantiate price comparison provider with id: {0}! Cause: {1}")]
    InstantiatePriceComparisonProvider(Arc<str>, Box<dyn StdError + Send + 'static>),
    #[error("Price comparison guard failure! Cause: {0}")]
    PriceComparisonGuard(#[from] PriceComparisonGuardError),
    #[error("Failed to serialize price feed message as JSON! Cause: {0}")]
    SerializeExecuteMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Recovery mode state watch closed!")]
    RecoveryModeWatchClosed,
}
