use std::error::Error as StdError;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Provider {
    #[error("Failed to join task into worker's own task! Cause: {0}")]
    TaskSetJoin(#[from] tokio::task::JoinError),

    #[error("URL operation failed! Cause: {0}")]
    UrlOperationFailed(#[from] url::ParseError),

    #[error("Failed to fetch price from pool! Cause: {0}")]
    FetchPoolPrice(reqwest::Error),

    #[error(
        "Failed to fetch price from pool for pair \"{0}/{1}\" because server responded with an error! Returned status code: {2}"
    )]
    ServerResponse(String, String, u16),

    #[error("Failed to deserialize fetched price from response's body! Cause: {0}")]
    DeserializePoolPrice(reqwest::Error),

    #[error("Failed to query WASM contract! Cause: {0}")]
    WasmQuery(#[from] chain_comms::interact::error::WasmQuery),

    #[error("Serialization failed! Cause: {0}")]
    Serialization(#[from] serde_json_wasm::ser::Error),
}

#[derive(Debug, Error)]
pub(crate) enum PriceComparisonGuard {
    #[error("Failed to fetch prices from provider for price comparison guard! Cause: {0}")]
    FetchPrices(Provider),
    #[error("Price comparison guard failed due to a duplicated price! Duplicated pair: {0}/{1}")]
    DuplicatePrice(String, String),
    #[error(
        "Price comparison guard failed due to a missing comparison price! Missing pair: {0}/{1}"
    )]
    MissingComparisonPrice(String, String),
    #[error("Price deviation too big for \"{0}/{1}\" pair! Deviation equal to {2} percent!")]
    DeviationTooBig(String, String, crate::deviation::UInt),
    #[error("Failure due to an provider-specific error! Cause: {0}")]
    ComparisonProviderSpecific(Box<dyn StdError + Send + 'static>),
}
