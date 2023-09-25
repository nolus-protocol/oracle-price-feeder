use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed to decode base-64 string! Cause: {0}")]
    DecodeBase64(#[from] base64::DecodeError),
    #[error("Failed to deserialize response data from Protobuf format! Cause: {0}")]
    DeserializeData(#[from] cosmrs::proto::prost::DecodeError),
    #[error("Failed to deserialize response data because returned data's type didn't match expected one! Cause: {0}")]
    InvalidResponseType(#[from] cosmrs::tx::ErrorReport),
}
