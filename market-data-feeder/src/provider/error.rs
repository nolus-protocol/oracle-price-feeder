use std::error::Error as StdError;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Provider {
    #[error("Failed to join task into worker's own task! Cause: {0}")]
    TaskSetJoin(#[from] tokio::task::JoinError),
    #[error("URL operation failed! Cause: {0}")]
    UrlOperationFailed(#[from] url::ParseError),
    #[error("Healthcheck failed! Cause: {0}")]
    Healthcheck(#[from] chain_comms::interact::healthcheck::error::Error),
    #[error(
        "Response contains non-ASCII characters!{}{_0}",
        if _0.is_empty() { "" } else { " Additional context: " },
    )]
    NonAsciiResponse(String),
    #[error(
        "Failed to parse price!{}{_0}{} Cause: {_1}",
        if _0.is_empty() { "" } else { " Additional context: " },
        if _0.is_empty() { "" } else { ";" },
    )]
    ParsePrice(String, crate::price::Error),
    #[error(
        "Failed to query WASM contract!{}{_0}{} Cause: {_1}",
        if _0.is_empty() { "" } else { " Additional context: " },
        if _0.is_empty() { "" } else { ";" },
    )]
    WasmQuery(String, chain_comms::interact::query::error::Wasm),
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
