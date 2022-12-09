use cosmrs::{
    proto::cosmos::base::v1beta1::Coin as CoinProto, tendermint::chain::Id as ChainId, Coin,
};
use serde::{de::Error as DeserializeError, Deserialize, Deserializer, Serialize};
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
    json_rpc_protocol: Protocol,
    grpc_protocol: Protocol,
    host: String,
    json_rpc_port: u16,
    #[serde(default)]
    json_rpc_api_path: Option<String>,
    grpc_port: u16,
    #[serde(default)]
    grpc_api_path: Option<String>,
    address_prefix: String,
    #[serde(deserialize_with = "deserialize_chain_id")]
    chain_id: ChainId,
    #[serde(deserialize_with = "deserialize_coin")]
    fee: Coin,
    gas_limit_per_alarm: u64,
}

fn deserialize_chain_id<'de, D>(deserializer: D) -> Result<ChainId, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse()
        .map_err(DeserializeError::custom)
}

fn deserialize_coin<'de, D>(deserializer: D) -> Result<Coin, D::Error>
where
    D: Deserializer<'de>,
{
    <CoinDTO as Deserialize>::deserialize(deserializer)
        .map(|coin| CoinProto {
            denom: coin.denom,
            amount: coin.amount,
        })?
        .try_into()
        .map_err(D::Error::custom)
}

impl Node {
    pub fn json_rpc_protocol(&self) -> Protocol {
        self.json_rpc_protocol
    }

    pub fn grpc_protocol(&self) -> Protocol {
        self.grpc_protocol
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn json_rpc_port(&self) -> u16 {
        self.json_rpc_port
    }

    pub fn json_rpc_api_path(&self) -> Option<&str> {
        self.json_rpc_api_path.as_deref()
    }

    pub fn grpc_port(&self) -> u16 {
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

    pub fn fee(&self) -> &Coin {
        &self.fee
    }

    pub fn gas_limit_per_alarm(&self) -> u64 {
        self.gas_limit_per_alarm
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[must_use]
pub struct Contract {
    address: String,
    max_alarms_group: u32,
}

impl Contract {
    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn max_alarms_group(&self) -> u32 {
        self.max_alarms_group
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct Config {
    poll_period_seconds: u64,
    node: Node,
    time_alarms: Contract,
    market_price_oracle: Contract,
}

impl Config {
    pub const fn poll_period_seconds(&self) -> u64 {
        self.poll_period_seconds
    }

    pub const fn node(&self) -> &Node {
        &self.node
    }

    pub const fn time_alarms(&self) -> &Contract {
        &self.time_alarms
    }

    pub const fn market_price_oracle(&self) -> &Contract {
        &self.market_price_oracle
    }
}

pub async fn read_config() -> ModuleResult<Config> {
    toml::from_str(&read_to_string("alarms-dispatcher.toml").await?).map_err(Into::into)
}
