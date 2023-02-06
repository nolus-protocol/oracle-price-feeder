use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use chain_comms::config::Node;

mod currencies;

pub type Ticker = String;

pub type Symbol = String;

#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct Config {
    continuous: bool,
    tick_time: u64,
    providers: Vec<Providers>,
    oracle_addr: String,
    gas_limit: u64,
    node: Node,
}

impl Config {
    #[cfg(test)]
    pub fn new(
        continuous: bool,
        tick_time: u64,
        providers: Vec<Providers>,
        oracle_addr: String,
        gas_limit: u64,
        node: Node,
    ) -> Self {
        Self {
            continuous,
            tick_time,
            providers,
            oracle_addr,
            gas_limit,
            node,
        }
    }

    pub fn continuous(&self) -> bool {
        self.continuous
    }
    pub fn tick_time(&self) -> u64 {
        self.tick_time
    }
    pub fn providers(&self) -> &[Providers] {
        &self.providers
    }
    pub fn oracle_addr(&self) -> &str {
        &self.oracle_addr
    }
    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }
}

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct Providers {
    pub main_type: String,
    pub name: String,
    pub base_address: String,
    #[serde(with = "currencies::serde")]
    pub currencies: BTreeMap<Ticker, Symbol>,
}
