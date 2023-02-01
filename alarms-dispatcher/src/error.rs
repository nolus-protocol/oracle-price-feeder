use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Application {
    #[error("Couldn't register global default tracing dispatcher! Cause: {0}")]
    SettingGlobalLogDispatcher(#[from] tracing::dispatcher::SetGlobalDefaultError),
    #[error("Setting up RPC environment failed! Cause: {0}")]
    RpcSetup(#[from] RpcSetup),
    #[error("Alarms dispatcher loop exited unexpectedly! Cause: {0}")]
    DispatchAlarms(#[from] DispatchAlarms),
}

pub type Result<T> = std::result::Result<T, Application>;

#[derive(Debug, ThisError)]
pub enum RpcSetup {
    #[error("Failed to resolve signing key! Cause: {0}")]
    SigningKey(#[from] SigningKey),
    #[error("Failed to load application configuration! Cause: {0}")]
    Configuration(#[from] alarms_dispatcher::configuration::error::Error),
    #[error("Failed to set up RPC client! Cause: {0}")]
    RpcClient(#[from] alarms_dispatcher::client::error::Error),
    #[error("Failed to resolve account ID! Cause: {0}")]
    AccountId(#[from] alarms_dispatcher::account::error::AccountId),
    #[error("Failed to resolve account state data! Cause: {0}")]
    AccountData(#[from] alarms_dispatcher::account::error::AccountData),
}

#[derive(Debug, ThisError)]
pub enum SigningKey {
    #[error("Couldn't read secret mnemonic from the standard input! Cause: {0}")]
    ReadingMnemonic(#[from] tokio::io::Error),
    #[error("Invalid mnemonic passed or is not in English! Cause: {0}")]
    ParsingMnemonic(cosmrs::bip32::Error),
    #[error("Couldn't parse derivation path! Cause: {0}")]
    ParsingDerivationPath(cosmrs::bip32::Error),
    #[error("Couldn't derive signing key! Cause: {0}")]
    DerivingKey(cosmrs::bip32::Error),
}

#[derive(Debug, ThisError)]
pub enum DispatchAlarms {
    #[error("Failed to serialize query message as JSON! Cause: {0}")]
    SerializeQueryMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Failed to dispatch time alarm! Cause: {0}")]
    DispatchTimeAlarm(DispatchAlarm),
    #[error("Failed to dispatch price alarm! Cause: {0}")]
    DispatchPriceAlarm(DispatchAlarm),
}

#[derive(Debug, ThisError)]
pub enum DispatchAlarm {
    #[error("Failed to query smart contract! Cause: {0}")]
    StatusQuery(#[from] StatusQuery),
    #[error("Failed to commit transaction! Cause: {0}")]
    TxCommit(#[from] TxCommit),
}

#[derive(Debug, ThisError)]
pub enum StatusQuery {
    #[error("Connection failure occurred! Cause: {0}")]
    Connection(#[from] tonic::Status),
    #[error("Failed to deserialize smart contract's query response from JSON! Cause: {0}")]
    DeserializeResponse(#[from] serde_json_wasm::de::Error),
}

#[derive(Debug, ThisError)]
pub enum TxCommit {
    #[error("Failed serializing execution message as JSON! Cause: {0}")]
    SerializeExecuteMessage(#[from] serde_json_wasm::ser::Error),
    #[error("Attempt to run simulation resulted in an error! Cause: {0}")]
    SimulationRunError(#[from] tonic::Status),
    #[error("Simulation result is missing gas into!")]
    MissingSimulationGasInto,
    #[error("Failed committing and signing execution message! Cause: {0}")]
    Commit(#[from] alarms_dispatcher::tx::error::Error),
    #[error("Failed to broadcast committed message! Cause: {0}")]
    Broadcast(#[from] cosmrs::ErrorReport),
    #[error("Failed serializing execution response message from JSON! Cause: {0}")]
    DeserializeExecutionResponse(#[from] serde_json_wasm::de::Error),
}
