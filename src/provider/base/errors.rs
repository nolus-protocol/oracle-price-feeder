use thiserror::Error;

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

    #[error("Request error. Cause: {message}")]
    RequestError { message: String },
}
