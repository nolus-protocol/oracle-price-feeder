#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use std::{
    collections::btree_map::{self, BTreeMap},
    mem::replace,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context as _, Result};
use tokio::{
    sync::Mutex,
    time::{sleep, Instant},
};

use ::task::{spawn_new, spawn_restarting, RunnableState};
use chain_ops::{
    node::{self, QueryTx},
    signer::Gas,
    tx::ExecuteTemplate,
};
use channel::{bounded, unbounded, Channel};
use contract::{
    Protocol, ProtocolDex, ProtocolProviderAndContracts, UncheckedContract,
};
use dex::{Dex, providers::ProviderType};
use environment::ReadFromVar as _;
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use supervisor::supervisor;
use task_set::TaskSet;
use tx::{TimeBasedExpiration, TxPackage};

use self::{
    dex_node_grpc_var::dex_node_grpc_var, id::Id, oracle::Oracle, state::State,
    task::TaskWithProvider,
};

mod dex_node_grpc_var;
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
                    false,
                )
                .await
            },
            remove_price_fetcher,
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
) -> impl for<'r> AsyncFnOnce(
    &'r mut TaskSet<Id, Result<()>>,
    bounded::Sender<Command>,
) -> Result<State> {
    async move |task_set, action_tx| {
        let state = State::new(service, transaction_rx, action_tx)?;

        spawn_new(task_set, state.balance_reporter().clone());

        spawn_new(task_set, state.broadcaster().clone());

        spawn_new(task_set, state.protocol_watcher().clone());

        Ok(state)
    }
}

#[inline]
fn error_handler(
    transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
) -> impl AsyncFnMut(&mut TaskSet<Id, Result<()>>, State, Id) -> Result<State> + use<>
{
    let mut task_states = BTreeMap::new();

    async move |task_set, mut state, id| -> Result<State> {
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
                let &error_handler::State {
                    non_delayed_task_retries_count,
                    failed_retry_margin,
                } = state.error_handler();

                let delayed = match task_states.entry(name.clone()) {
                    btree_map::Entry::Vacant(entry) => {
                        entry.insert((
                            Instant::now(),
                            non_delayed_task_retries_count,
                        ));

                        false
                    },
                    btree_map::Entry::Occupied(ref mut entry) => {
                        let (instant, retries) = entry.get_mut();

                        let now = Instant::now();

                        *retries = if now.duration_since(replace(instant, now))
                            < failed_retry_margin
                        {
                            retries.saturating_sub(1)
                        } else {
                            non_delayed_task_retries_count
                        };

                        *retries == 0
                    },
                };

                tracing::info!(
                    protocol = %name,
                    "Restarting price fetcher{}.",
                    if delayed { " with delay" } else { "" },
                );

                state = spawn_price_fetcher(
                    task_set,
                    state,
                    name,
                    &transaction_tx,
                    delayed,
                )
                .await
                .context("Failed to spawn price fetcher task!")?;

                task_states.retain(|_, (instant, _)| {
                    instant.elapsed() < failed_retry_margin
                });
            },
        }

        Ok(state)
    }
}

#[must_use]
pub(crate) struct PriceFetcher {
    pub name: Arc<str>,
    pub dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
    pub idle_duration: Duration,
    pub signer_address: Arc<str>,
    pub hard_gas_limit: Gas,
    pub transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
    pub query_tx: QueryTx,
    pub timeout_duration: Duration,
}

impl PriceFetcher {
    pub async fn run<Dex>(
        self,
        oracle: UncheckedContract<contract::Oracle<Dex>>,
        provider_network: String,
        provider: Dex,
    ) -> Result<()>
    where
        Dex: self::Dex<ProviderTypeDescriptor = ProviderType>,
    {
        let dex_node_client = {
            let client = {
                let guard = self.dex_node_clients.clone().lock_owned().await;

                guard.get(&*provider_network).cloned()
            };

            if let Some(client) = client {
                client
            } else {
                let client = node::Client::connect(&String::read_from_var(
                    dex_node_grpc_var(provider_network.clone()),
                )?)
                .await
                .context("Failed to connect to node's gRPC endpoint!")?;

                self.dex_node_clients
                    .lock_owned()
                    .await
                    .entry(provider_network.into_boxed_str())
                    .or_insert(client)
                    .clone()
            }
        };

        let oracle = ::oracle::Oracle::new(oracle)
            .await
            .context("Failed to connect to oracle contract!")?;

        let source = format!(
            "{dex}; Protocol: {name}",
            dex = Dex::PROVIDER_TYPE,
            name = self.name,
        )
        .into();

        TaskWithProvider {
            protocol: self.name,
            source,
            query_tx: self.query_tx,
            dex_node_client,
            duration_before_start: Duration::default(),
            execute_template: ExecuteTemplate::new(
                (&*self.signer_address).into(),
                oracle.address().into(),
            ),
            idle_duration: self.idle_duration,
            timeout_duration: self.timeout_duration,
            hard_gas_limit: self.hard_gas_limit,
            oracle: Oracle::new(oracle, Duration::from_secs(15))
                .await
                .context("Failed to fetch oracle contract data!")?,
            provider,
            transaction_tx: self.transaction_tx,
        }
        .run(RunnableState::New)
        .await
        .context("Price fetcher task errored!")
    }
}

async fn spawn_price_fetcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    name: Arc<str>,
    transaction_tx: &unbounded::Sender<TxPackage<TimeBasedExpiration>>,
    delayed: bool,
) -> Result<State> {
    struct TaskSpawner<'r> {
        task_set: &'r mut TaskSet<Id, Result<()>>,
        id: Id,
        price_fetcher: PriceFetcher,
        network: String,
        delayed: bool,
    }

    impl TaskSpawner<'_> {
        fn spawn_with<Dex>(
            self,
            ProtocolProviderAndContracts { provider, oracle }: ProtocolProviderAndContracts<Dex>,
        ) where
            Dex: self::Dex<ProviderTypeDescriptor = ProviderType>,
        {
            let Self {
                task_set,
                id,
                price_fetcher,
                network,
                delayed,
            } = self;

            task_set.add_handle(
                id,
                if delayed {
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(15)).await;

                        price_fetcher.run(oracle, network, provider).await
                    })
                } else {
                    tokio::spawn(price_fetcher.run(oracle, network, provider))
                },
            );
        }
    }

    tracing::info!(%name, "Price fetcher is starting...");

    let state::PriceFetcher {
        mut admin_contract,
        dex_node_clients,
        idle_duration,
        signer_address,
        hard_gas_limit,
        query_tx,
        timeout_duration,
    } = state.price_fetcher().clone();

    let Protocol {
        network,
        provider_and_contracts,
    } = admin_contract.protocol(&name).await?;

    let task = TaskSpawner {
        task_set,
        id: Id::PriceFetcher {
            protocol: name.clone(),
        },
        price_fetcher: PriceFetcher {
            name,
            idle_duration,
            transaction_tx: transaction_tx.clone(),
            signer_address: signer_address.clone(),
            hard_gas_limit,
            query_tx,
            dex_node_clients: dex_node_clients.clone(),
            timeout_duration,
        },
        network,
        delayed,
    };

    match provider_and_contracts {
        ProtocolDex::Astroport(provider_and_contracts) => {
            task.spawn_with(provider_and_contracts);
        },
        ProtocolDex::Osmosis(provider_and_contracts) => {
            task.spawn_with(provider_and_contracts);
        },
    }

    Ok(state)
}

async fn remove_price_fetcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    protocol: Arc<str>,
) -> Result<State> {
    task_set.abort(&Id::PriceFetcher { protocol });

    Ok(state)
}
