use anyhow::Result as AnyResult;
use serde::{Deserialize, Serialize};
use tokio::fs::read_to_string;

use market_data_feeder::configuration::Oracle;

use crate::log_error;

#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct Config {
    pub max_alarms_in_transaction: u32,
    pub oracle: Oracle,
}

pub async fn read_config() -> AnyResult<Config> {
    log_error!(
        toml::from_str(&log_error!(
            read_to_string("alarms-dispatcher.toml").await,
            "Failed to read contents of configuration file!"
        )?),
        "Failed to parse configuration!"
    )
    .map_err(Into::into)
}
