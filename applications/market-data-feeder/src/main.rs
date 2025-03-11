#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use anyhow::{Context as _, Result};

use ::task::spawn_new;
use channel::{Channel, bounded, unbounded};
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use supervisor::supervisor;
use task_set::TaskSet;
use tx::{TimeBasedExpiration, TxPackage};

use self::{
    error_handler::error_handler,
    id::Id,
    state::State,
    task::{PriceFetcherRunnableState, spawn_price_fetcher},
};

mod dex_node_grpc_var;
mod error_handler;
mod id;
mod oracle;
mod state;
mod task;

#[tokio::main]
async fn main() -> Result<()> {
    log::init().context("Failed to initialize logging!")?;

    let service = Service::read_from_env()
        .await
        .context("Failed to load service configuration!")?;

    let (transaction_tx, transaction_rx) = unbounded::Channel::new();

    supervisor::<_, _, bounded::Channel<_>, _, _, _>(
        init_tasks(service, transaction_rx),
        protocol_watcher::action_handler(
            transaction_tx.clone(),
            async move |task_set, state, name, transaction_tx| {
                spawn_price_fetcher(
                    task_set,
                    state,
                    name,
                    transaction_tx,
                    PriceFetcherRunnableState::New,
                )
                .await
            },
            async move |task_set, state, protocol| {
                task_set.abort(&Id::PriceFetcher { protocol });

                Ok(state)
            },
        ),
        error_handler(transaction_tx),
    )
    .await
    .map(drop)
}

#[inline]
fn init_tasks(
    service: Service,
    transaction_rx: unbounded::Receiver<TxPackage<TimeBasedExpiration>>,
) -> impl for<'task_set> AsyncFnOnce(
    &'task_set mut TaskSet<Id, Result<()>>,
    bounded::Sender<Command>,
) -> Result<State>
+ use<> {
    async move |task_set, action_tx| {
        let state = State::new(service, transaction_rx, action_tx)?;

        spawn_new(task_set, state.balance_reporter().clone());

        spawn_new(task_set, state.broadcaster().clone());

        spawn_new(task_set, state.protocol_watcher().clone());

        Ok(state)
    }
}
