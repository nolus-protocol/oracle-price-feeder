use serde::{Deserialize, Serialize};

use chain_comms::config::Node;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Contract {
    address: String,
    max_alarms_group: u32,
    gas_limit_per_alarm: u64,
}

impl Contract {
    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn max_alarms_group(&self) -> u32 {
        self.max_alarms_group
    }

    pub fn gas_limit_per_alarm(&self) -> u64 {
        self.gas_limit_per_alarm
    }
}

#[derive(Debug, Deserialize)]
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

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}
