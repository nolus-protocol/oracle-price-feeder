use thiserror::Error;

#[derive(Error, Debug)]
#[error("Invalid type of provider encountered! Type: {0}")]
pub struct InvalidProviderType(String);

impl InvalidProviderType {
    pub(crate) fn new(input: String) -> Self {
        Self(input)
    }
}

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

    #[error("Failed to fetch price from pool! Cause: {0}")]
    FetchPoolPrice(reqwest::Error),

    #[error("Failed to deserialize fetched price from response's body! Cause: {0}")]
    DeserializePoolPrice(reqwest::Error),

    #[error("{0}")]
    WasmQueryError(#[from] chain_comms::interact::error::WasmQuery),

    #[error("{0}")]
    SerializationError(#[from] serde_json_wasm::ser::Error),
}
