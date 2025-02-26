#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use std::{collections::btree_map::BTreeMap, sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use tokio::sync::Mutex;

use chain_ops::{
    node::{self, QueryTx},
    signer::Gas,
    tx::ExecuteTemplate,
};
use channel::{bounded, unbounded, Channel};
use contract::{
    Admin, CheckedContract, Protocol, ProtocolDex,
    ProtocolProviderAndContracts, UncheckedContract,
};
use dex::{provider, providers::ProviderType};
use environment::ReadFromVar as _;
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use supervisor::supervisor;
use ::task::{spawn_new, spawn_restarting, RunnableState, Task};
use task_set::TaskSet;
use tx::{TimeBasedExpiration, TxPackage};

use self::{oracle::Oracle, state::State, task::TaskWithProvider};

mod oracle;
mod state;
mod task;

#[tokio::main]
async fn main() -> Result<()> {
    log::init().context("Failed to initialize logging!")?;

    let service = Service::read_from_env()
        .await
        .context("Failed to load service configuration!")?;

    let (tx, rx) = unbounded::Channel::new();

    supervisor::<_, _, bounded::Channel<_>, _, _, _>(
        init_tasks(service, rx),
        protocol_watcher::action_handler(
            tx.clone(),
            spawn_price_fetcher,
            remove_price_fetcher,
        ),
        error_handler(tx),
    )
    .await
    .map(drop)
}

impl Task<Id> for balance_reporter::State {
    const ID: Id = Id::BalanceReporter;
}

impl Task<Id> for broadcaster::State<TimeBasedExpiration> {
    const ID: Id = Id::Broadcaster;
}

impl Task<Id> for protocol_watcher::State {
    const ID: Id = Id::ProtocolWatcher;
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
                tracing::info!(%name, "Restarting price fetcher");

                state =
                    spawn_price_fetcher(task_set, state, name, &transaction_tx)
                        .await
                        .context("Failed to spawn price fetcher task!")?;
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
        Dex: provider::Dex<ProviderTypeDescriptor = ProviderType>,
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
            name = self.name
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

#[derive(Clone)]
#[must_use]
struct PriceFetcherState {
    pub admin_contract: CheckedContract<Admin>,
    pub dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
    pub idle_duration: Duration,
    pub signer_address: Arc<str>,
    pub hard_gas_limit: Gas,
    pub query_tx: QueryTx,
    pub timeout_duration: Duration,
}

async fn spawn_price_fetcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    name: Arc<str>,
    transaction_tx: &unbounded::Sender<TxPackage<TimeBasedExpiration>>,
) -> Result<State> {
    tracing::info!(%name, "Price fetcher is starting...");

    let PriceFetcherState {
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

    let price_fetcher = PriceFetcher {
        name: name.clone(),
        idle_duration,
        transaction_tx: transaction_tx.clone(),
        signer_address: signer_address.clone(),
        hard_gas_limit,
        query_tx,
        dex_node_clients: dex_node_clients.clone(),
        timeout_duration,
    };

    task_set.add_handle(
        Id::PriceFetcher { protocol: name },
        match provider_and_contracts {
            ProtocolDex::Astroport(ProtocolProviderAndContracts {
                provider,
                oracle,
            }) => tokio::spawn(price_fetcher.run(oracle, network, provider)),
            ProtocolDex::Osmosis(ProtocolProviderAndContracts {
                provider,
                oracle,
            }) => tokio::spawn(price_fetcher.run(oracle, network, provider)),
        },
    );

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Id {
    BalanceReporter,
    Broadcaster,
    ProtocolWatcher,
    PriceFetcher { protocol: Arc<str> },
}

const SEPARATOR_CHAR: char = '_';

const SEPARATOR_STR: &str = {
    const BYTES: [u8; SEPARATOR_CHAR.len_utf8()] = {
        let mut bytes = [0; SEPARATOR_CHAR.len_utf8()];

        SEPARATOR_CHAR.encode_utf8(&mut bytes);

        bytes
    };

    if let Ok(s) = core::str::from_utf8(&BYTES) {
        s
    } else {
        panic!("Separator should be valid UTF-8!")
    }
};

const VAR_SUFFIX: &str = {
    const SEGMENTS: &[&str] = &["NODE", "GRPC"];

    const LENGTH: usize = {
        let mut sum = (SEGMENTS.len() + 1) * SEPARATOR_STR.len();

        let mut index = 0;

        while index < SEGMENTS.len() {
            sum += SEGMENTS[index].len();

            index += 1;
        }

        sum
    };

    const BYTES: [u8; LENGTH] = {
        const fn write_bytes(
            destination: &mut [u8; LENGTH],
            mut destination_index: usize,
            source: &[u8],
        ) -> usize {
            let mut source_index = 0;

            while source_index < source.len() {
                destination[destination_index] = source[source_index];

                destination_index += 1;

                source_index += 1;
            }

            destination_index
        }

        #[inline]
        const fn write_separator(
            destination: &mut [u8; LENGTH],
            index: usize,
        ) -> usize {
            write_bytes(destination, index, SEPARATOR_STR.as_bytes())
        }

        let mut bytes = [0; LENGTH];

        let mut byte_index = write_separator(&mut bytes, 0);

        let mut index = 0;

        while index < SEGMENTS.len() {
            byte_index = write_separator(&mut bytes, byte_index);

            byte_index =
                write_bytes(&mut bytes, byte_index, SEGMENTS[index].as_bytes());

            index += 1;
        }

        bytes
    };

    if let Ok(s) = core::str::from_utf8(&BYTES) {
        s
    } else {
        panic!("Environment variable name suffix should be valid UTF-8!")
    }
};

fn dex_node_grpc_var(mut network: String) -> String {
    network.make_ascii_uppercase();

    if const { SEPARATOR_CHAR != '-' } {
        while let Some(index) = network.find('-') {
            network.replace_range(index..=index, SEPARATOR_STR);
        }
    }

    network.reserve_exact(VAR_SUFFIX.len());

    network.push_str(VAR_SUFFIX);

    network
}

#[test]
fn test_f() {
    assert_eq!(VAR_SUFFIX, "__NODE_GRPC");

    assert_eq!(
        dex_node_grpc_var("AbBCD_e-Fg-H-i".into()),
        "ABBCD_E_FG_H_I__NODE_GRPC"
    );
}
