use std::time::Duration;

use chain_comms::config::{read_from_env, ReadFromEnvError};

#[derive(Debug, Copy, Clone)]
#[must_use]
pub struct Config {
    tick_time: Duration,
    poll_time: Duration,
    between_tx_margin_time: Duration,
}

impl Config {
    pub fn read_from_env() -> Result<Self, ReadFromEnvError> {
        Ok(Self {
            tick_time: Duration::from_secs(read_from_env("TICK_TIME_SECONDS")?),
            poll_time: Duration::from_secs(read_from_env("POLL_TIME_SECONDS")?),
            between_tx_margin_time: Duration::from_secs(read_from_env(
                "BETWEEN_TX_MARGIN_SECONDS",
            )?),
        })
    }

    #[must_use]
    pub const fn tick_time(&self) -> Duration {
        self.tick_time
    }

    #[must_use]
    pub const fn poll_time(&self) -> Duration {
        self.poll_time
    }

    #[must_use]
    pub const fn between_tx_margin_time(&self) -> Duration {
        self.between_tx_margin_time
    }
}
