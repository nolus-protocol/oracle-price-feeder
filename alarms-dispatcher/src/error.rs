use thiserror::Error as ThisError;

use semver::SemVer;

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
    #[error("Version of \"{contract}\" contract is not compatible! Minimum compatible version is {minimum_compatible}, but contract's actual version is {actual}!")]
    IncompatibleContractVersion {
        contract: &'static str,
        minimum_compatible: SemVer,
        actual: SemVer,
    },
    #[error("Alarms dispatcher loop exited unexpectedly! Cause: {0}")]
    DispatchAlarms(#[from] DispatchAlarms),
}

pub type AppResult<T> = Result<T, Application>;

#[derive(Debug, ThisError)]
pub enum DispatchAlarms {
    #[error("Failed to serialize query message as JSON! Cause: {0}")]
    SerializeQueryMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Failed to dispatch time alarm! Cause: {0}")]
    DispatchTimeAlarm(DispatchAlarm),
    #[error("Failed to dispatch price alarm! Cause: {0}")]
    DispatchPriceAlarm(DispatchAlarm),
    #[error("Failed to recover after error occurred!")]
    RecoveryError,
}

#[derive(Debug, ThisError)]
pub enum DispatchAlarm {
    #[error("Failed to query smart contract! Cause: {0}")]
    StatusQuery(#[from] chain_comms::interact::error::WasmQuery),
    #[error("Failed to commit transaction! Cause: {0}")]
    CommitTx(#[from] CommitDispatchTx),
    #[error("Failed to deserialize dispatch response! Cause: {0}")]
    DeserializeDispatchResponse(#[from] serde_json_wasm::de::Error),
}

#[derive(Debug, ThisError)]
pub enum CommitDispatchTx {
    #[error("Failed to serialize dispatch message as JSON! Cause: {0}")]
    SerializeDispatchMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Failed to commit transaction! Cause: {0}")]
    CommitTx(#[from] chain_comms::interact::error::GasEstimatingTxCommit),
    #[error("Failed to deserialize response data! Cause: {0}")]
    DeserializeTxData(#[from] chain_comms::decode::error::Error),
    #[error(r#"Failed to deserialize dispatch response! Cause: {0}; Data: "{1}""#)]
    DeserializeDispatchResponse(serde_json_wasm::de::Error, String),
    #[error(r#"Transaction failed! Cause: {0}"#)]
    TxFailed(String),
}
