use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Failed to read contents of configuration file! Cause: {0}")]
    FileRead(#[from] std::io::Error),
    #[error("Failed to parse configuration! Cause: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, ThisError)]
#[error(r#"Unknown protocol: "{0}"!"#)]
pub struct InvalidProtocol(pub(super) String);

pub type Result<T> = std::result::Result<T, Error>;
