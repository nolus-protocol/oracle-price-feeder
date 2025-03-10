use std::time::Duration;

use anyhow::Result;

use environment::ReadFromVar;

pub struct Environment {
    pub non_delayed_task_retries_count: u8,
    pub failed_retry_margin: Duration,
}

impl Environment {
    pub fn read_from_env() -> Result<Self> {
        ReadFromVar::read_from_var("NON_DELAYED_TASK_RETRIES_COUNT").and_then(
            |non_delayed_task_retries_count| {
                ReadFromVar::read_from_var("FAILED_RETRY_MARGIN")
                    .map(Duration::from_secs)
                    .map(|failed_retry_margin| Self {
                        non_delayed_task_retries_count,
                        failed_retry_margin,
                    })
            },
        )
    }
}

#[derive(Clone)]
#[must_use]
pub struct State {
    pub non_delayed_task_retries_count: u8,
    pub failed_retry_margin: Duration,
}
