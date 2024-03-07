use std::{
    env::{var, VarError},
    num::{NonZeroU32, NonZeroU64},
};

use serde::{Deserialize, Serialize};

use broadcast::config::Config as BroadcastConfig;
use chain_comms::config::Node as NodeConfig;

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(try_from = "File")]
pub(crate) struct Config {
    pub admin_contract: Box<str>,
    pub broadcast: BroadcastConfig,
    pub node: NodeConfig,
    pub time_alarms: AlarmsConfig,
    pub market_price_oracle: AlarmsConfig,
}

impl AsRef<NodeConfig> for Config {
    fn as_ref(&self) -> &NodeConfig {
        &self.node
    }
}

impl TryFrom<File> for Config {
    type Error = VarError;

    fn try_from(value: File) -> Result<Self, Self::Error> {
        match var("OVERRIDE_ADMIN_CONTRACT").map(String::into_boxed_str) {
            Ok(admin_contract) => Ok(Self {
                admin_contract,
                broadcast: value.broadcast,
                node: value.node,
                time_alarms: value.time_alarms,
                market_price_oracle: value.market_price_oracle,
            }),
            Err(VarError::NotPresent) => Ok(Self {
                admin_contract: value.admin_contract,
                broadcast: value.broadcast,
                node: value.node,
                time_alarms: value.time_alarms,
                market_price_oracle: value.market_price_oracle,
            }),
            Err(error @ VarError::NotUnicode { .. }) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct AlarmsConfig {
    pub max_alarms_group: NonZeroU32,
    pub gas_limit_per_alarm: NonZeroU64,
}

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct File {
    pub admin_contract: Box<str>,
    pub broadcast: BroadcastConfig,
    pub node: NodeConfig,
    pub time_alarms: AlarmsConfig,
    pub market_price_oracle: AlarmsConfig,
}
