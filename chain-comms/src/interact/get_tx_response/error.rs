use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error(
        "Error occurred while communicating with RPC endpoint! Cause: {0}"
    )]
    Rpc(#[from] tonic::Status),
    #[error("Query completed but didn't no response was found! Broadcast may have failed!")]
    EmptyResponseReceived,
}
