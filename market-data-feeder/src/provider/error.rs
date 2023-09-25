use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to join task into worker's own task! Cause: {0}")]
    TaskSetJoin(#[from] tokio::task::JoinError),

    #[error("URL operation failed! Cause: {0}")]
    UrlOperationFailed(#[from] url::ParseError),

    #[error("Failed to fetch price from pool! Cause: {0}")]
    FetchPoolPrice(reqwest::Error),

    #[error(
        "Failed to fetch price from pool for pair \"{0}/{1}\" because server responded with an error! Returned status code: {0}"
    )]
    ServerResponse(String, String, u16),

    #[error("Failed to deserialize fetched price from response's body! Cause: {0}")]
    DeserializePoolPrice(reqwest::Error),

    #[error("{0}")]
    WasmQuery(#[from] chain_comms::interact::error::WasmQuery),

    #[error("{0}")]
    Serialization(#[from] serde_json_wasm::ser::Error),
}
