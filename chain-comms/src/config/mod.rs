use std::{
    env::{var, VarError},
    error::Error as StdError,
    num::{NonZeroU16, NonZeroU64, NonZeroUsize},
    path::Path,
    str::FromStr,
};

use cosmrs::tendermint::chain::Id as ChainId;
use serde::{
    de::{DeserializeOwned, Error as DeserializeError},
    Deserialize, Deserializer, Serialize,
};
use tokio::fs::read_to_string;

use self::error::{InvalidProtocol, Result as ModuleResult};

pub mod error;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Http,
    Https,
}

impl FromStr for Protocol {
    type Err = InvalidProtocol;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let protocol: String = s.to_ascii_lowercase();

        match protocol.as_str() {
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            _ => Err(InvalidProtocol(protocol)),
        }
    }
}

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
    pub fn http2_concurrency_limit(&self) -> Option<NonZeroUsize> {
        self.file.http2_concurrency_limit
    }

    pub fn json_rpc_protocol(&self) -> Protocol {
        self.environment.json_rpc_protocol
    }

    pub fn grpc_protocol(&self) -> Protocol {
        self.environment.grpc_protocol
    }

    pub fn json_rpc_host(&self) -> &str {
        &self.environment.json_rpc_host
    }

    pub fn grpc_host(&self) -> &str {
        &self.environment.grpc_host
    }

    pub fn json_rpc_port(&self) -> NonZeroU16 {
        self.environment.json_rpc_port
    }

    pub fn json_rpc_api_path(&self) -> Option<&str> {
        self.environment.json_rpc_api_path.as_deref()
    }

    pub fn grpc_port(&self) -> NonZeroU16 {
        self.environment.grpc_port
    }

    pub fn grpc_api_path(&self) -> Option<&str> {
        self.environment.grpc_api_path.as_deref()
    }

    pub fn address_prefix(&self) -> &str {
        &self.file.address_prefix
    }

    pub fn chain_id(&self) -> &ChainId {
        &self.file.chain_id
    }

    pub fn fee_denom(&self) -> &str {
        &self.file.fee_denom
    }

    pub fn gas_adjustment_numerator(&self) -> NonZeroU64 {
        self.file.gas_adjustment_numerator
    }

    pub fn gas_adjustment_denominator(&self) -> NonZeroU64 {
        self.file.gas_adjustment_denominator
    }

    pub fn gas_price_numerator(&self) -> NonZeroU64 {
        self.file.gas_price_numerator
    }

    pub fn gas_price_denominator(&self) -> NonZeroU64 {
        self.file.gas_price_denominator
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

pub async fn read_config<C, P>(path: P) -> ModuleResult<C>
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
}

#[derive(Debug, Clone)]
#[must_use]
struct Environment {
    json_rpc_protocol: Protocol,
    grpc_protocol: Protocol,
    json_rpc_host: String,
    grpc_host: String,
    json_rpc_port: NonZeroU16,
    grpc_port: NonZeroU16,
    json_rpc_api_path: Option<String>,
    grpc_api_path: Option<String>,
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D>(_: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_rpc_protocol: Protocol = read_from_env::<'de, _, D>("JSON_RPC_PROTO")?;
        let grpc_protocol: Protocol = read_from_env::<'de, _, D>("GRPC_PROTO")?;
        let json_rpc_host: String = read_from_env::<'de, _, D>("JSON_RPC_HOST")?;
        let grpc_host: String = read_from_env::<'de, _, D>("GRPC_HOST")?;
        let json_rpc_port: NonZeroU16 = read_from_env::<'de, _, D>("JSON_RPC_PORT")?;
        let grpc_port: NonZeroU16 = read_from_env::<'de, _, D>("GRPC_PORT")?;
        let json_rpc_api_path: Option<String> =
            maybe_read_from_env::<'de, _, D>("JSON_RPC_API_PATH")?;
        let grpc_api_path: Option<String> = maybe_read_from_env::<'de, _, D>("GRPC_API_PATH")?;

        Ok(Self {
            json_rpc_protocol,
            grpc_protocol,
            json_rpc_host,
            grpc_host,
            json_rpc_port,
            grpc_port,
            json_rpc_api_path,
            grpc_api_path,
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
            r#"Value for environment variable "{}" contains invalid unicode data."#,
            var_name
        ))),
    }
}
