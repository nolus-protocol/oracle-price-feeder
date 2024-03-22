use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error(
        "Failed to encode one or more of transaction messages! Cause: {0}"
    )]
    EncodingMessage(#[from] prost::EncodeError),
    #[error("Signing transaction failed! Cause: {0}")]
    Signer(#[from] crate::signer::error::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
