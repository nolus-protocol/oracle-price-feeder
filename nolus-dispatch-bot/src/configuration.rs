use std::process::exit;

use serde::{Deserialize, Serialize};
use tracing::error;

use market_data_feeder::configuration::Oracle;

#[derive(Debug, Serialize, Deserialize)]
#[must_use]
pub struct Config {
    pub max_alarms_in_transaction: u32,
    pub oracle: Oracle,
}

pub fn read_config() -> Config {
    std::fs::read_to_string("alarms-dispatcher.toml")
        .and_then(|content| toml::from_str(&content).map_err(Into::into))
        .unwrap_or_else(|error| {
            error!(
                error = %error,
                "Couldn't read configuration from file!"
            );

            exit(1);
        })
}
