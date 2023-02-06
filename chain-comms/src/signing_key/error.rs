use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Couldn't read secret mnemonic from the standard input! Cause: {0}")]
    ReadingMnemonic(#[from] tokio::io::Error),
    #[error("Invalid mnemonic passed or is not in English! Cause: {0}")]
    ParsingMnemonic(cosmrs::bip32::Error),
    #[error("Couldn't parse derivation path! Cause: {0}")]
    ParsingDerivationPath(cosmrs::bip32::Error),
    #[error("Couldn't derive signing key! Cause: {0}")]
    DerivingKey(cosmrs::bip32::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
