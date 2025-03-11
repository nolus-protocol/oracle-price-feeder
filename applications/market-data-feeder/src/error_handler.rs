use std::time::Duration;

use anyhow::{Context as _, Result};

use channel::unbounded;
use error_handler::RestartStrategy;
use task::spawn_restarting;
use task_set::TaskSet;
use tx::{TimeBasedExpiration, TxPackage};

use crate::{id::Id, state::State, task::PriceFetcherRunnableState};

#[inline]
pub fn error_handler(
    transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
) -> impl for<'task_set> AsyncFnMut(
    &'task_set mut TaskSet<Id, Result<()>>,
    State,
    Id,
) -> Result<State>
+ use<> {
    async move |task_set, mut state: State, id| -> Result<State> {
        match id {
            Id::BalanceReporter {} => {
                spawn_restarting(task_set, state.balance_reporter().clone());
            },
            Id::Broadcaster {} => {
                spawn_restarting(task_set, state.broadcaster().clone());
            },
            Id::ProtocolWatcher {} => {
                spawn_restarting(task_set, state.protocol_watcher().clone());
            },
            Id::PriceFetcher { protocol: name } => {
                let restart_strategy =
                    state.error_handler_mut().restart_strategy(name.clone());

                tracing::info!(
                    protocol = %name,
                    "Restarting price fetcher{}.",
                    if matches!(restart_strategy, RestartStrategy::Immediate) { "" } else { " with delay" },
                );

                state = super::spawn_price_fetcher(
                    task_set,
                    state,
                    name,
                    &transaction_tx,
                    if matches!(restart_strategy, RestartStrategy::Immediate) {
                        PriceFetcherRunnableState::ImmediateRestart
                    } else {
                        PriceFetcherRunnableState::DelayedRestart(
                            Duration::from_secs(15),
                        )
                    },
                )
                .await
                .context("Failed to spawn price fetcher task!")?;
            },
        }

        Ok(state)
    }
}
