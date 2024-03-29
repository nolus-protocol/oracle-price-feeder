use std::error::Error as StdError;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Provider {
    #[error("Failed to join task into worker's own task! Cause: {0}")]
    TaskSetJoin(#[from] tokio::task::JoinError),

    #[error("URL operation failed! Cause: {0}")]
    UrlOperationFailed(#[from] url::ParseError),

    #[error("Response contains non-ASCII characters!{}{}", if _0.is_empty() { "" } else { " Additional context: " }, _0)]
    NonAsciiResponse(String),

    #[error("Failed to parse price!{}{}{} Cause: {}", if _0.is_empty() { "" } else { " Additional context: " }, _0, if _0.is_empty() { "" } else { ";" }, _1)]
    ParsePrice(String, crate::price::Error),

    #[error(r#"Failed to query WASM contract!{}{}{} Cause: {}"#, if _0.is_empty() { "" } else { " Additional context: " }, _0, if _0.is_empty() { "" } else { ";" }, _1)]
    WasmQuery(String, chain_comms::interact::query::error::Wasm),

    #[error("Serialization failed! Cause: {0}")]
    Serialization(#[from] serde_json_wasm::ser::Error),
}

impl From<chain_comms::interact::query::error::Wasm> for Provider {
    fn from(error: chain_comms::interact::query::error::Wasm) -> Self {
        Self::WasmQuery(String::new(), error)
    }
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
