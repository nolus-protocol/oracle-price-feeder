use std::time::Duration;

use serde::{de::Deserializer, Deserialize};

#[derive(Debug, Deserialize)]
pub struct Config {
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
}

fn deserialize_duration_in_seconds<'de, D>(
    deserializer: D,
) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    u64::deserialize(deserializer).map(Duration::from_secs)
}
