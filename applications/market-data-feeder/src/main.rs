#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use std::{collections::btree_map::BTreeMap, sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use tokio::sync::Mutex;

use ::task::{
    RunnableState, spawn_new, spawn_restarting, spawn_restarting_delayed,
};
use chain_ops::{
    node::{self, QueryTx},
    signer::Gas,
    tx::ExecuteTemplate,
};
use channel::{Channel, bounded, unbounded};
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
    dex_node_grpc_var::dex_node_grpc_var, error_handler::error_handler, id::Id,
    oracle::Oracle, state::State, task::TaskWithProvider,
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
                    RunnableState::New,
                    false,
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
    ) -> Result<TaskWithProvider<Dex>>
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

        Ok(TaskWithProvider {
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
        })
    }
}

async fn spawn_price_fetcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    name: Arc<str>,
    transaction_tx: &unbounded::Sender<TxPackage<TimeBasedExpiration>>,
    runnable_state: RunnableState,
    delayed: bool,
) -> Result<State> {
    struct TaskSpawner<'r> {
        task_set: &'r mut TaskSet<Id, Result<()>>,
        price_fetcher: PriceFetcher,
        network: String,
        runnable_state: RunnableState,
        delayed: bool,
    }

    impl TaskSpawner<'_> {
        async fn spawn_with<Dex>(
            self,
            ProtocolProviderAndContracts { provider, oracle }: ProtocolProviderAndContracts<Dex>,
        ) -> Result<()>
        where
            Dex: self::Dex<ProviderTypeDescriptor = ProviderType>,
        {
            let Self {
                task_set,
                price_fetcher,
                network,
                runnable_state,
                delayed,
            } = self;

            let task = price_fetcher.run(oracle, network, provider).await?;

            if matches!(runnable_state, RunnableState::New) {
                spawn_new(task_set, task);
            } else {
                if delayed {
                    spawn_restarting_delayed(
                        task_set,
                        task,
                        Duration::from_secs(15),
                    );
                } else {
                    spawn_restarting(task_set, task);
                }
            }

            Ok(())
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
        runnable_state,
        delayed,
    };

    match provider_and_contracts {
        ProtocolDex::Astroport(provider_and_contracts) => {
            task.spawn_with(provider_and_contracts).await
        },
        ProtocolDex::Osmosis(provider_and_contracts) => {
            task.spawn_with(provider_and_contracts).await
        },
    }
    .map(|()| state)
}
