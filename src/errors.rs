use thiserror::Error;

#[derive(Error, Debug)]
pub enum FeederError {
    #[error("Configuration error: {message}")]
    ConfigurationError { message: String },

    #[error("Invalid contract address {address}")]
    InvalidOracleContractAddress { address: String },

    #[error("Authentication error. Cause {message}")]
    AuthError { message: String },

    #[error("{0}")]
    ReqwestError(#[from] reqwest::Error),
}
