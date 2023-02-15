use std::{
    num::{NonZeroU16, NonZeroU64, NonZeroUsize},
    path::Path,
};

use cosmrs::tendermint::chain::Id as ChainId;
use serde::{
    de::{DeserializeOwned, Error as DeserializeError},
    Deserialize, Deserializer, Serialize,
};
use tokio::fs::read_to_string;

use self::error::Result as ModuleResult;

pub mod error;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Http,
    Https,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CoinDTO {
    amount: String,
    denom: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub struct Node {
    #[serde(default)]
    http2_concurrency_limit: Option<NonZeroUsize>,
    json_rpc_protocol: Protocol,
    grpc_protocol: Protocol,
    json_rpc_host: String,
    grpc_host: String,
    json_rpc_port: NonZeroU16,
    #[serde(default)]
    json_rpc_api_path: Option<String>,
    grpc_port: NonZeroU16,
    #[serde(default)]
    grpc_api_path: Option<String>,
    address_prefix: String,
    #[serde(deserialize_with = "deserialize_chain_id")]
    chain_id: ChainId,
    fee_denom: String,
    gas_adjustment_numerator: NonZeroU64,
    gas_adjustment_denominator: NonZeroU64,
    gas_price_numerator: NonZeroU64,
    gas_price_denominator: NonZeroU64,
}

impl Node {
    pub fn http2_concurrency_limit(&self) -> Option<NonZeroUsize> {
        self.http2_concurrency_limit
    }

    pub fn json_rpc_protocol(&self) -> Protocol {
        self.json_rpc_protocol
    }

    pub fn grpc_protocol(&self) -> Protocol {
        self.grpc_protocol
    }

    pub fn json_rpc_host(&self) -> &str {
        &self.json_rpc_host
    }

    pub fn grpc_host(&self) -> &str {
        &self.grpc_host
    }

    pub fn json_rpc_port(&self) -> NonZeroU16 {
        self.json_rpc_port
    }

    pub fn json_rpc_api_path(&self) -> Option<&str> {
        self.json_rpc_api_path.as_deref()
    }

    pub fn grpc_port(&self) -> NonZeroU16 {
        self.grpc_port
    }

    pub fn grpc_api_path(&self) -> Option<&str> {
        self.grpc_api_path.as_deref()
    }

    pub fn address_prefix(&self) -> &str {
        &self.address_prefix
    }

    pub fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }

    pub fn fee_denom(&self) -> &str {
        &self.fee_denom
    }

    pub fn gas_adjustment_numerator(&self) -> NonZeroU64 {
        self.gas_adjustment_numerator
    }

    pub fn gas_adjustment_denominator(&self) -> NonZeroU64 {
        self.gas_adjustment_denominator
    }

    pub fn gas_price_numerator(&self) -> NonZeroU64 {
        self.gas_price_numerator
    }

    pub fn gas_price_denominator(&self) -> NonZeroU64 {
        self.gas_price_denominator
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
