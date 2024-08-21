use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context as _, Result};
use cosmrs::Gas;

use chain_ops::{env::ReadFromVar, node};

pub struct ApplicationDefined {
    pub(super) dex_node_clients: BTreeMap<String, node::Client>,
    pub(super) duration_before_start: Duration,
    pub(super) gas_limit: Gas,
    pub(super) update_currencies_interval: Duration,
}

impl ApplicationDefined {
    pub fn new() -> Result<Self> {
        Ok(ApplicationDefined {
            dex_node_clients: BTreeMap::new(),
            duration_before_start: read_duration_before_start()?,
            gas_limit: read_gas_limit()?,
            update_currencies_interval: read_update_currencies_interval()?,
        })
    }
}

fn read_duration_before_start() -> Result<Duration> {
    u64::read_from_var("DURATION_BEFORE_START")
        .map(Duration::from_secs)
        .context("Failed to read duration before feeding starts!")
}

fn read_gas_limit() -> Result<Gas> {
    Gas::read_from_var("GAS_LIMIT").context("Failed to read gas limit!")
}

fn read_update_currencies_interval() -> Result<Duration> {
    u64::read_from_var("UPDATE_CURRENCIES_INTERVAL_SECONDS")
        .map(Duration::from_secs)
        .context("Failed to read update currencies interval!")
}
