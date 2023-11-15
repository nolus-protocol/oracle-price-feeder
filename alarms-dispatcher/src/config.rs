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
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    #[must_use]
    pub fn max_alarms_group(&self) -> u32 {
        self.max_alarms_group
    }

    #[must_use]
    pub fn gas_limit_per_alarm(&self) -> u64 {
        self.gas_limit_per_alarm
    }
}

#[derive(Debug, Deserialize)]
#[must_use]
pub struct Config {
    poll_period_seconds: u64,
    node: Node,
    time_alarms: Vec<Contract>,
    market_price_oracle: Vec<Contract>,
}

impl Config {
    #[must_use]
    pub const fn poll_period_seconds(&self) -> u64 {
        self.poll_period_seconds
    }

    pub const fn node(&self) -> &Node {
        &self.node
    }

    pub fn time_alarms(&self) -> &[Contract] {
        &self.time_alarms
    }

    pub fn market_price_oracle(&self) -> &[Contract] {
        &self.market_price_oracle
    }
}

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}
