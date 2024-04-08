use std::{
    env::{var, VarError},
    fmt::Display,
    num::{NonZeroU64, NonZeroUsize},
    str::FromStr,
};

use cosmrs::Denom;

pub mod error;

#[derive(Debug, Clone)]
#[must_use]
pub struct Node {
    grpc_uri: Box<str>,
    http2_concurrency_limit: Option<NonZeroUsize>,
    address_prefix: Box<str>,
    fee_denom: Denom,
    gas_adjustment_numerator: NonZeroU64,
    gas_adjustment_denominator: NonZeroU64,
    gas_price_numerator: NonZeroU64,
    gas_price_denominator: NonZeroU64,
    fee_adjustment_numerator: NonZeroU64,
    fee_adjustment_denominator: NonZeroU64,
}

impl Node {
    pub fn read_from_env() -> Result<Self, ReadFromEnvError> {
        Ok(Self {
            grpc_uri: read_from_env::<String>("GRPC_URI")?.into_boxed_str(),
            http2_concurrency_limit: maybe_read_from_env(
                "HTTP2_CONCURRENCY_LIMIT",
            )?,
            address_prefix: read_from_env::<String>("ADDRESS_PREFIX")?
                .into_boxed_str(),
            fee_denom: read_from_env("FEE_DENOM")?,
            gas_adjustment_numerator: read_from_env(
                "GAS_ADJUSTMENT_NUMERATOR",
            )?,
            gas_adjustment_denominator: read_from_env(
                "GAS_ADJUSTMENT_DENOMINATOR",
            )?,
            gas_price_numerator: read_from_env("GAS_PRICE_NUMERATOR")?,
            gas_price_denominator: read_from_env("GAS_PRICE_DENOMINATOR")?,
            fee_adjustment_numerator: read_from_env(
                "FEE_ADJUSTMENT_NUMERATOR",
            )?,
            fee_adjustment_denominator: read_from_env(
                "FEE_ADJUSTMENT_DENOMINATOR",
            )?,
        })
    }

    #[must_use]
    pub const fn http2_concurrency_limit(&self) -> Option<NonZeroUsize> {
        self.http2_concurrency_limit
    }

    #[must_use]
    pub const fn grpc_uri(&self) -> &str {
        &self.grpc_uri
    }

    #[must_use]
    pub const fn address_prefix(&self) -> &str {
        &self.address_prefix
    }

    #[must_use]
    pub const fn fee_denom(&self) -> &Denom {
        &self.fee_denom
    }

    #[must_use]
    pub const fn gas_adjustment_numerator(&self) -> NonZeroU64 {
        self.gas_adjustment_numerator
    }

    #[must_use]
    pub const fn gas_adjustment_denominator(&self) -> NonZeroU64 {
        self.gas_adjustment_denominator
    }

    #[must_use]
    pub const fn gas_price_numerator(&self) -> NonZeroU64 {
        self.gas_price_numerator
    }

    #[must_use]
    pub const fn gas_price_denominator(&self) -> NonZeroU64 {
        self.gas_price_denominator
    }

    #[must_use]
    pub const fn fee_adjustment_numerator(&self) -> NonZeroU64 {
        self.fee_adjustment_numerator
    }

    #[must_use]
    pub const fn fee_adjustment_denominator(&self) -> NonZeroU64 {
        self.fee_adjustment_denominator
    }
}

impl AsRef<Self> for Node {
    fn as_ref(&self) -> &Self {
        self
    }
}

pub fn maybe_read_from_env<T>(
    var_name: &'static str,
) -> Result<Option<T>, ReadFromEnvError>
where
    T: FromStr,
    T::Err: ToString,
{
    match var(var_name) {
        Ok(value) => T::from_str(&value).map(Some).map_err(|error| {
            ReadFromEnvInnerError::Parse {
                error: error.to_string().into_boxed_str(),
            }
        }),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(ReadFromEnvInnerError::NotUnicode),
    }
    .map_err(|error| ReadFromEnvError {
        key: var_name.to_string().into_boxed_str(),
        error,
    })
}

pub fn read_from_env<T>(var_name: &'static str) -> Result<T, ReadFromEnvError>
where
    T: FromStr,
    T::Err: Display,
{
    maybe_read_from_env::<T>(var_name).and_then(|maybe_value| {
        maybe_value.ok_or_else(|| ReadFromEnvError {
            key: var_name.to_string().into_boxed_str(),
            error: ReadFromEnvInnerError::NotPresent,
        })
    })
}

#[derive(Debug, thiserror::Error)]
#[error(r#"Failed to read environment variable with key {key:?}! {error}"#)]
pub struct ReadFromEnvError {
    key: Box<str>,
    error: ReadFromEnvInnerError,
}

#[derive(Debug, thiserror::Error)]
enum ReadFromEnvInnerError {
    #[error("Variable doesn't contain proper UTF-8 value!")]
    NotUnicode,
    #[error("Variable is not present!")]
    NotPresent,
    #[error("Failure to parse value! {error}")]
    Parse { error: Box<str> },
}
