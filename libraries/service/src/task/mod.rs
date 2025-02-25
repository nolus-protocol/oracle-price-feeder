use std::{error::Error, future::Future};

use anyhow::Result;

use task::RunnableState;

use crate::service::task_spawner::CancellationToken;

pub mod application_defined;
pub mod balance_reporter;
pub mod broadcast;
pub mod protocol_watcher;

pub trait Runnable: Sized {
    fn run(
        self,
        state: RunnableState,
    ) -> impl Future<Output = Result<()>> + Send;
}

#[must_use]
pub struct State {
    _cancellation_token: CancellationToken,
    retry: u8,
}

impl State {
    const fn new(cancellation_token: CancellationToken) -> Self {
        Self {
            _cancellation_token: cancellation_token,
            retry: 0,
        }
    }

    fn replace_and_increment(&mut self, cancellation_token: CancellationToken) {
        *self = Self {
            _cancellation_token: cancellation_token,
            retry: self.retry.saturating_add(1),
        };
    }

    #[must_use]
    pub fn retry(&self) -> u8 {
        self.retry
    }
}
