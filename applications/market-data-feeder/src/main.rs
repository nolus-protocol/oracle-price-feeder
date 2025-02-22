#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use anyhow::Result;
use chain_ops::node;
use chain_ops::node::QueryTx;
use chain_ops::signer::Gas;
use chain_ops::tx::ExecuteTemplate;
use channel::{bounded, unbounded};
use channel::{Channel, Sender};
use contract::{
    Admin, CheckedContract, Protocol, ProtocolDex,
    ProtocolProviderAndContracts, UncheckedContract,
};
use dex::provider;
use dex::providers::ProviderType;
use market_data_feeder::task::TaskWithProvider;
use service::supervisor::configuration::Service;
use service::supervisor::new_supervisor;
use service::task::balance_reporter::BalanceReporter;
use service::task::broadcast::Broadcast;
use service::task::protocol_watcher::{Command, ProtocolWatcher};
use service::task::{Runnable, RunnableState, TimeBasedExpiration, TxPackage};
use std::collections::{
    btree_map::{self, BTreeMap},
    BTreeSet,
};
use std::sync::Arc;
use std::time::Duration;
use task_set::TaskSet;
use tokio::sync::Mutex;

fn init_tasks(
    service: Service,
    rx: unbounded::Receiver<TxPackage<TimeBasedExpiration>>,
) -> impl for<'r> AsyncFnOnce(
    &'r mut TaskSet<Id, Result<()>>,
    bounded::Sender<Command>,
) -> Result<State> {
    let state = State {
        admin_contract: service.admin_contract.clone(),
        dex_node_clients: Arc::new(Mutex::new(BTreeMap::new())),
        idle_duration: service.idle_duration,
        signer_address: service.signer.address().into(),
        hard_gas_limit: 0,
        query_tx: service.node_client.clone().query_tx(),
        timeout_duration: service.timeout_duration,
    };

    async move |task_set, action_tx| {
        task_set.add_handle(
            Id::BalanceReporter,
            tokio::spawn(
                BalanceReporter::new(
                    service.node_client.clone().query_bank(),
                    service.signer.address().into(),
                    service.signer.fee_token().into(),
                    service.balance_reporter_idle_duration,
                )
                .run(RunnableState::New),
            ),
        );

        task_set.add_handle(
            Id::Broadcaster,
            tokio::spawn(
                Broadcast::<TimeBasedExpiration>::new(
                    service.node_client.broadcast_tx(),
                    service.signer,
                    rx,
                    service.broadcast_delay_duration,
                    service.broadcast_retry_delay_duration,
                )
                .run(RunnableState::New),
            ),
        );

        task_set.add_handle(
            Id::ProtocolWatcher,
            tokio::spawn(
                ProtocolWatcher::new(
                    service.admin_contract,
                    BTreeSet::new(),
                    action_tx,
                )
                .run(RunnableState::New),
            ),
        );

        Ok(state)
    }
}

fn protocol_watcher<TxSender, TaskSpawnerF>(
    tx: TxSender,
    mut task_spawner: TaskSpawnerF,
) -> impl for<'r> AsyncFnMut(
    &'r mut TaskSet<Id, Result<()>>,
    State,
    Command,
) -> Result<State>
where
    TxSender: Sender<Value = TxPackage<TimeBasedExpiration>>,
    TaskSpawnerF: for<'r, 't> AsyncFnMut(
        &'r TxSender,
        &'t mut TaskSet<Id, Result<()>>,
        State,
        Arc<str>,
        Protocol,
    ) -> Result<State>,
{
    async move |task_set, mut state, command| match command {
        Command::ProtocolAdded(name) => {
            let protocol = state.admin_contract.protocol(&name).await?;

            task_spawner(&tx, task_set, state, name, protocol).await
        },
        Command::ProtocolRemoved(protocol) => {
            task_set.abort(&Id::PriceFetcher { protocol });

            Ok(state)
        },
    }
}

async fn spawn_price_fetcher(
    transaction_tx: &unbounded::Sender<TxPackage<TimeBasedExpiration>>,
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    name: Arc<str>,
    Protocol {
        provider_and_contracts,
        ..
    }: Protocol,
) -> Result<State> {
    let price_fetcher = PriceFetcher {
        name: name.clone(),
        idle_duration: state.idle_duration,
        transaction_tx: transaction_tx.clone(),
        signer_address: state.signer_address.clone(),
        hard_gas_limit: state.hard_gas_limit,
        query_tx: state.query_tx.clone(),
        dex_node_clients: state.dex_node_clients.clone(),
        timeout_duration: state.timeout_duration,
    };

    task_set.add_handle(
        Id::PriceFetcher { protocol: name },
        match provider_and_contracts {
            ProtocolDex::Astroport(ProtocolProviderAndContracts {
                provider,
                oracle,
            }) => tokio::spawn(price_fetcher.run(oracle, provider)),
            ProtocolDex::Osmosis(ProtocolProviderAndContracts {
                provider,
                oracle,
            }) => tokio::spawn(price_fetcher.run(oracle, provider)),
        },
    );

    Ok(state)
}

#[tokio::main]
async fn main() -> Result<()> {
    let service = Service::read_from_env().await?;

    let (tx, rx) = unbounded::Channel::new();

    new_supervisor::<_, _, bounded::Channel<_>, _, _, _>(
        init_tasks(service, rx),
        protocol_watcher(tx, spawn_price_fetcher),
        async |task_set: &mut TaskSet<Id, Result<()>>,
               state: State,
               id: Id|
               -> Result<State> { Ok(state) },
    )
    .await
    .map(drop)
}

pub(crate) struct PriceFetcher {
    pub name: Arc<str>,
    pub dex_node_clients: Arc<Mutex<BTreeMap<ProviderType, node::Client>>>,
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
        provider: Dex,
    ) -> Result<()>
    where
        Dex: provider::Dex<ProviderTypeDescriptor = ProviderType>,
    {
        let mut oracle = oracle::Oracle::new(oracle).await?;

        let dex_node_client = match self
            .dex_node_clients
            .lock_owned()
            .await
            .entry(Dex::PROVIDER_TYPE)
        {
            btree_map::Entry::Vacant(entry) => {
                todo!()
            },
            btree_map::Entry::Occupied(entry) => entry.get().clone(),
        };

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
            duration_before_start: Default::default(),
            execute_template: ExecuteTemplate::new(
                (&*self.signer_address).into(),
                (&*oracle.address()).into(),
            ),
            idle_duration: self.idle_duration,
            timeout_duration: self.timeout_duration,
            hard_gas_limit: self.hard_gas_limit,
            oracle: market_data_feeder::oracle::Oracle::new(
                oracle,
                Duration::from_secs(15),
            )
            .await?,
            provider,
            transaction_tx: self.transaction_tx,
        }
        .run(RunnableState::New)
        .await
    }
}

struct State {
    admin_contract: CheckedContract<Admin>,
    dex_node_clients: Arc<Mutex<BTreeMap<ProviderType, node::Client>>>,
    idle_duration: Duration,
    signer_address: Arc<str>,
    hard_gas_limit: Gas,
    query_tx: QueryTx,
    timeout_duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Id {
    BalanceReporter,
    Broadcaster,
    ProtocolWatcher,
    PriceFetcher { protocol: Arc<str> },
}

// run_app!(
//     task_creation_context: {
//         ApplicationDefinedContext::new()
//     },
//     startup_tasks: [] as [Id; 0],
// );
