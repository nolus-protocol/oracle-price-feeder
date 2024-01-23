use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum GetTxResponse {
    #[error("Error occured while communicating with RPC endpoint! Cause: {0}")]
    Rpc(#[from] cosmrs::rpc::Error),
}
