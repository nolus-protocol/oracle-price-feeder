use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Signing of transaction data failed! Cause: {0}")]
    Signing(#[from] cosmrs::ErrorReport),
}

pub type Result<T> = std::result::Result<T, Error>;
