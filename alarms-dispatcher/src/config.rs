use std::num::{NonZeroU32, NonZeroU64};

use broadcast::config::Config as BroadcastConfig;
use chain_comms::config::{
    read_from_env, Node as NodeConfig, ReadFromEnvError,
};

#[derive(Debug, Clone)]
#[must_use]
pub(crate) struct Config {
    pub admin_contract: Box<str>,
    pub broadcast: BroadcastConfig,
    pub node: NodeConfig,
    pub time_alarms: AlarmsConfig,
    pub market_price_oracle: AlarmsConfig,
}

impl Config {
    pub fn read_from_env() -> Result<Self, ReadFromEnvError> {
        Ok(Self {
            admin_contract: read_from_env::<String>("ADMIN_CONTRACT")?
                .into_boxed_str(),
            broadcast: BroadcastConfig::read_from_env()?,
            node: NodeConfig::read_from_env()?,
            time_alarms: AlarmsConfig {
                max_alarms_group: read_from_env(
                    "TIME_ALARMS_MAX_ALARMS_GROUP",
                )?,
                gas_limit_per_alarm: read_from_env(
                    "TIME_ALARMS_GAS_LIMIT_PER_ALARM",
                )?,
            },
            market_price_oracle: AlarmsConfig {
                max_alarms_group: read_from_env(
                    "PRICE_ALARMS_MAX_ALARMS_GROUP",
                )?,
                gas_limit_per_alarm: read_from_env(
                    "PRICE_ALARMS_GAS_LIMIT_PER_ALARM",
                )?,
            },
        })
    }
}

impl AsRef<NodeConfig> for Config {
    fn as_ref(&self) -> &NodeConfig {
        &self.node
    }
}

#[derive(Debug, Clone, Copy)]
#[must_use]
pub(crate) struct AlarmsConfig {
    pub max_alarms_group: NonZeroU32,
    pub gas_limit_per_alarm: NonZeroU64,
}
