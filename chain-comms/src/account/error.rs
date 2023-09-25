use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
#[error("Failed to derive account ID from public key! Cause: {0}")]
pub struct AccountId(#[from] pub cosmrs::ErrorReport);

#[derive(Debug, ThisError)]
pub enum AccountData {
    #[error("Querying node for account data failed because of connection error! Cause: {0}")]
    Query(#[from] tonic::Status),
    #[error("No data is associated with account on the node!")]
    NotFound,
    #[error("Failed decoding account data from response! Cause: {0}")]
    Decoding(#[from] cosmrs::proto::prost::DecodeError),
}

pub type AccountIdResult<T> = Result<T, AccountId>;

pub type AccountDataResult<T> = Result<T, AccountData>;
