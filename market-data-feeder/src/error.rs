use std::error::Error as StdError;

use thiserror::Error as ThisError;

use semver::SemVer;

use crate::deviation::UInt as DeviationPercentInt;

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
    #[error("Contract's version is not compatible! Minimum compatible version is {minimum_compatible}, but contract's actual version is {actual}!")]
    IncompatibleContractVersion {
        minimum_compatible: SemVer,
        actual: SemVer,
    },
    #[error("Unknown provider identifier! Got: {0}")]
    UnknownProviderId(String),
    #[error("Unknown price comparison provider identifier! Got: {0}")]
    UnknownPriceComparisonProviderId(String),
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
    InstantiatePriceComparisonProvider(String, Box<dyn StdError + Send + 'static>),
    #[error("Price comparison guard failure! Cause: {0}")]
    PriceComparisonGuard(#[from] PriceComparisonGuard),
    #[error("Failed to serialize price feed message as JSON! Cause: {0}")]
    SerializeExecuteMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Recovery mode state watch closed!")]
    RecoveryModeWatchClosed,
}

#[derive(Debug, ThisError)]
pub(crate) enum PriceComparisonGuard {
    #[error("Failed to fetch prices for price comparison guard! Cause: {0}")]
    FetchPrices(crate::provider::Error),
    #[error("Failed to fetch comparison prices for price comparison guard! Cause: {0}")]
    FetchComparisonPrices(crate::provider::Error),
    #[error("Price comparison guard failed due to a duplicated price! Duplicated pair: {0}")]
    DuplicatePrice(String, String),
    #[error("Price comparison guard failed due to a missing comparison price! Missing pair: {0}")]
    MissingComparisonPrice(String, String),
    #[error("Price deviation too big for \"{0}/{1}\" pair! Deviation equal to {2} percent!")]
    DeviationTooBig(String, String, DeviationPercentInt),
}
