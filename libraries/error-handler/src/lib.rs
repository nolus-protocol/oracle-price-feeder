use std::{
    collections::btree_map::{BTreeMap, Entry},
    mem::replace,
    time::Duration,
};

use anyhow::Result;
use tokio::time::Instant;

use environment::ReadFromVar;

pub struct Environment {
    non_delayed_task_retries_count: u8,
    failed_retry_margin: Duration,
}

impl Environment {
    pub fn read_from_env() -> Result<Self> {
        let non_delayed_task_retries_count =
            ReadFromVar::read_from_var("NON_DELAYED_TASK_RETRIES_COUNT")?;

        let failed_retry_margin =
            ReadFromVar::read_from_var("FAILED_RETRY_MARGIN")
                .map(Duration::from_secs)?;

        Ok(Self {
            non_delayed_task_retries_count,
            failed_retry_margin,
        })
    }
}

#[derive(Clone)]
#[must_use]
pub struct State<Id> {
    non_delayed_task_retries_count: u8,
    failed_retry_margin: Duration,
    states: BTreeMap<Id, (Instant, u8)>,
}

impl<Id> State<Id> {
    #[inline]
    pub const fn new(
        Environment {
            non_delayed_task_retries_count,
            failed_retry_margin,
        }: Environment,
    ) -> Self {
        Self {
            non_delayed_task_retries_count,
            failed_retry_margin,
            states: BTreeMap::new(),
        }
    }
}

impl<Id> State<Id>
where
    Id: Ord,
{
    pub fn restart_strategy(&mut self, name: Id) -> RestartStrategy {
        let now = Instant::now();

        let immediate_retries_left = match self.states.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert((now, self.non_delayed_task_retries_count)).1
            },
            Entry::Occupied(ref mut entry) => {
                let (instant, retries) = entry.get_mut();

                let duration_since_last_restart =
                    now.duration_since(replace(instant, now));

                *retries =
                    if duration_since_last_restart < self.failed_retry_margin {
                        retries.saturating_sub(1)
                    } else {
                        self.non_delayed_task_retries_count
                    };

                *retries
            },
        };

        self.cleanup_stale();

        if immediate_retries_left == 0 {
            RestartStrategy::Delayed
        } else {
            RestartStrategy::Immediate
        }
    }

    fn cleanup_stale(&mut self) {
        self.states.retain(|_, (instant, _)| {
            instant.elapsed() < self.failed_retry_margin
        });
    }
}

pub enum RestartStrategy {
    Immediate,
    Delayed,
}
