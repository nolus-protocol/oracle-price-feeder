use thiserror::Error;

use crate::cosmos::error::CosmosError;

#[derive(Error, Debug)]
pub enum FeedProviderError {
    #[error("Asset pair not found in pool")]
    AssetPairNotFound,

    #[error("Denom {denom} not found")]
    DenomNotFound { denom: String },

    #[error("Invalid poll. Empty weight")]
    InvalidPoolEmptyWeight,

    #[error("No price found for pair [ {base} / {quote} ]")]
    NoPriceFound { base: String, quote: String },

    #[error("No prices to push")]
    NoPrices,

    #[error("Request error. Cause: {message}")]
    RequestError { message: String },

    #[error("Invalid provider url {0}")]
    InvalidProviderURL(String),

    #[error("URL parsing error")]
    URLParsingError,

    #[error("Unexpected error")]
    UnexpectedError,

    #[error("Unsupported provider type {0}")]
    UnsupportedProviderType(String),

    #[error("{0}")]
    ReqwestError(#[from] reqwest::Error),

    #[error("{0}")]
    CosmosError(#[from] CosmosError),

    #[error("{0}")]
    Json(#[from] serde_json::Error),
}
