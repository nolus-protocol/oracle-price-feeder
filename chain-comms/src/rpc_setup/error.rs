use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed to resolve signing key! Cause: {0}")]
    SigningKey(#[from] crate::signing_key::error::Error),
    #[error("Failed to load application configuration! Cause: {0}")]
    Configuration(#[from] crate::config::error::Error),
    #[error("Failed to set up RPC client! Cause: {0}")]
    RpcClient(#[from] crate::client::error::Error),
    #[error("Failed to resolve account ID! Cause: {0}")]
    AccountId(#[from] crate::account::error::AccountId),
    #[error("Failed to resolve account state data! Cause: {0}")]
    AccountData(#[from] crate::account::error::AccountData),
}

pub type Result<T> = std::result::Result<T, Error>;
