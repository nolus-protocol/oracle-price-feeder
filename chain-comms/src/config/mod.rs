use std::{
    env::{var, VarError},
    error::Error as StdError,
    num::{NonZeroU64, NonZeroUsize},
    path::Path,
    str::FromStr,
};

use cosmrs::tendermint::chain::Id as ChainId;
use serde::{
    de::{DeserializeOwned, Error as DeserializeError},
    Deserialize, Deserializer, Serialize,
};
use tokio::fs::read_to_string;

use self::error::Result as ModuleResult;

pub mod error;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct CoinDTO {
    amount: String,
    denom: String,
}

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Node {
    #[serde(flatten)]
    file: File,
    #[serde(flatten)]
    environment: Environment,
}

impl Node {
    #[must_use]
    pub const fn http2_concurrency_limit(&self) -> Option<NonZeroUsize> {
        self.file.http2_concurrency_limit
    }

    #[must_use]
    pub fn json_rpc_url(&self) -> &str {
        &self.environment.json_rpc_url
    }

    #[must_use]
    pub fn grpc_url(&self) -> &str {
        &self.environment.grpc_url
    }

    #[must_use]
    pub fn address_prefix(&self) -> &str {
        &self.file.address_prefix
    }

    #[must_use]
    pub fn chain_id(&self) -> &ChainId {
        &self.file.chain_id
    }

    #[must_use]
    pub fn fee_denom(&self) -> &str {
        &self.file.fee_denom
    }

    #[must_use]
    pub const fn gas_adjustment_numerator(&self) -> NonZeroU64 {
        self.file.gas_adjustment_numerator
    }

    #[must_use]
    pub const fn gas_adjustment_denominator(&self) -> NonZeroU64 {
        self.file.gas_adjustment_denominator
    }

    #[must_use]
    pub const fn gas_price_numerator(&self) -> NonZeroU64 {
        self.file.gas_price_numerator
    }

    #[must_use]
    pub const fn gas_price_denominator(&self) -> NonZeroU64 {
        self.file.gas_price_denominator
    }

    #[must_use]
    pub const fn fee_adjustment_numerator(&self) -> NonZeroU64 {
        self.file.fee_adjustment_numerator
    }

    #[must_use]
    pub const fn fee_adjustment_denominator(&self) -> NonZeroU64 {
        self.file.fee_adjustment_denominator
    }
}

impl AsRef<Self> for Node {
    fn as_ref(&self) -> &Self {
        self
    }
}

fn deserialize_chain_id<'de, D>(deserializer: D) -> Result<ChainId, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(DeserializeError::custom)
}

pub async fn read<C, P>(path: P) -> ModuleResult<C>
where
    C: DeserializeOwned + AsRef<Node>,
    P: AsRef<Path>,
{
    toml::from_str(&read_to_string(path).await?).map_err(Into::into)
}

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct File {
    #[serde(default)]
    http2_concurrency_limit: Option<NonZeroUsize>,
    address_prefix: String,
    #[serde(deserialize_with = "deserialize_chain_id")]
    chain_id: ChainId,
    fee_denom: String,
    gas_adjustment_numerator: NonZeroU64,
    gas_adjustment_denominator: NonZeroU64,
    gas_price_numerator: NonZeroU64,
    gas_price_denominator: NonZeroU64,
    fee_adjustment_numerator: NonZeroU64,
    fee_adjustment_denominator: NonZeroU64,
}

#[derive(Debug, Clone)]
#[must_use]
struct Environment {
    json_rpc_url: String,
    grpc_url: String,
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D>(_: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_rpc_url: String = read_from_env::<'de, _, D>("JSON_RPC_URL")?;
        let grpc_url: String = read_from_env::<'de, _, D>("GRPC_URL")?;

        Ok(Self {
            json_rpc_url,
            grpc_url,
        })
    }
}

fn read_from_env<'de, T, D>(var_name: &'static str) -> Result<T, D::Error>
where
    T: FromStr,
    T::Err: StdError,
    D: Deserializer<'de>,
{
    maybe_read_from_env::<'de, T, D>(var_name)
        .and_then(|maybe_value| maybe_value.ok_or(D::Error::missing_field(var_name)))
}

fn maybe_read_from_env<'de, T, D>(var_name: &'static str) -> Result<Option<T>, D::Error>
where
    T: FromStr,
    T::Err: StdError,
    D: Deserializer<'de>,
{
    match var(var_name) {
        Ok(value) => T::from_str(&value).map(Some).map_err(D::Error::custom),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(D::Error::custom(format!(
            r#"Value for environment variable "{var_name}" contains invalid unicode data."#,
        ))),
    }
}
