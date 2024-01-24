use std::num::{NonZeroU32, NonZeroU64};

use serde::{Deserialize, Serialize};

use broadcast::config::Config as BroadcastConfig;
use chain_comms::config::Node as NodeConfig;

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct Config {
    pub broadcast: BroadcastConfig,
    pub node: NodeConfig,
    pub time_alarms: Vec<Contract>,
    pub market_price_oracle: Vec<Contract>,
}

impl AsRef<NodeConfig> for Config {
    fn as_ref(&self) -> &NodeConfig {
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
