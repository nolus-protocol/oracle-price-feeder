use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed to decode transaction data! Cause: {0}")]
    Decode(#[from] data_encoding::DecodeError),
    #[error(
        "Failed to deserialize response data from Protobuf format! Cause: {0}"
    )]
    Deserialize(#[from] prost::DecodeError),
    #[error("Failed to deserialize response data because returned data's type didn't match expected one! Cause: {0}")]
    InvalidResponseType(#[from] cosmrs::tx::ErrorReport),
}
