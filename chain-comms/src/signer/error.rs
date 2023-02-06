use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
#[error("Signing of transaction data failed! Cause: {0}")]
pub struct Error(#[from] cosmrs::ErrorReport);

pub type Result<T> = std::result::Result<T, Error>;
