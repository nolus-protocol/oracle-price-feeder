use std::{
    num::{NonZeroU32, NonZeroU64},
    time::Duration,
};

use serde::{Deserialize, Deserializer, Serialize};

use chain_comms::config::Node;

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(
        rename = "tick_time_seconds",
        deserialize_with = "deserialize_duration_in_seconds"
    )]
    pub tick_time: Duration,
    #[serde(
        rename = "poll_time_seconds",
        deserialize_with = "deserialize_duration_in_seconds"
    )]
    pub poll_time: Duration,
    #[serde(
        rename = "between_tx_margin_seconds",
        deserialize_with = "deserialize_duration_in_seconds"
    )]
    pub between_tx_margin_time: Duration,
    pub node: Node,
    pub time_alarms: Vec<Contract>,
    pub market_price_oracle: Vec<Contract>,
}

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct Contract {
    pub address: String,
    pub max_alarms_group: NonZeroU32,
    pub gas_limit_per_alarm: NonZeroU64,
}

fn deserialize_duration_in_seconds<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    u64::deserialize(deserializer).map(Duration::from_secs)
}
