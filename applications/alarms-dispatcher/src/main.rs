#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use std::sync::Arc;

use anyhow::{Context as _, Result};
use tokio::sync::Mutex;

use ::task::{spawn_new, spawn_restarting, Run, RunnableState, Task};
use chain_ops::{
    node,
    signer::{Gas, Signer},
};
use channel::{bounded, unbounded, Channel as _};
use contract::{
    Admin, CheckedContract, GeneralizedProtocol, GeneralizedProtocolContracts,
    Platform,
};
use environment::ReadFromVar as _;
use protocol_watcher::Command;
use service::supervisor::configuration::Service;
use supervisor::supervisor;
use task_set::TaskSet;
use tx::{NoExpiration, TxPackage};

use self::task::{AlarmsGenerator, Configuration, PriceAlarms, TimeAlarms};

mod task;

#[tokio::main]
async fn main() -> Result<()> {
    log::init().context("Failed to initialize logging!")?;

    let service = Service::read_from_env()
        .await
        .context("Failed to load service configuration!")?;

    let (transaction_tx, transaction_rx) = unbounded::Channel::new();

    supervisor::<_, _, bounded::Channel<_>, _, _, _>(
        init_tasks(
            service,
            transaction_tx.clone(),
            transaction_rx,
            ApplicationDefinedContext {
                gas_per_time_alarm: read_gas_per_time_alarm()?,
                time_alarms_per_message: read_time_alarms_per_message()?,
                gas_per_price_alarm: read_gas_per_price_alarm()?,
                price_alarms_per_message: read_price_alarms_per_message()?,
            },
        ),
        protocol_watcher::action_handler(
            transaction_tx.clone(),
            add_price_alarm_dispatcher,
            remove_price_alarm_dispatcher,
        ),
        error_handler(transaction_tx),
    )
    .await
    .map(drop)
}

fn init_tasks(
    service: Service,
    transaction_tx: unbounded::Sender<TxPackage<NoExpiration>>,
    transaction_rx: unbounded::Receiver<TxPackage<NoExpiration>>,
    a: ApplicationDefinedContext,
) -> impl for<'r> AsyncFnOnce(
    &'r mut TaskSet<Id, Result<()>>,
    bounded::Sender<Command>,
) -> Result<State> {
    async move |task_set, action_tx| {
        let state =
            State::new(service, transaction_tx, transaction_rx, action_tx, a)
                .await?;

        spawn_new(task_set, state.balance_reporter.clone());

        spawn_new(task_set, state.broadcaster.clone());

        spawn_new(task_set, state.protocol_watcher.clone());

        spawn_new(task_set, state.time_alarms.clone());

        Ok(state)
    }
}

async fn add_price_alarm_dispatcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    mut state: State,
    name: Arc<str>,
    _transaction_tx: &unbounded::Sender<TxPackage<NoExpiration>>,
) -> Result<State> {
    let GeneralizedProtocol {
        contracts: GeneralizedProtocolContracts { oracle },
        ..
    } = state
        .admin_contract
        .generalized_protocol(&name)
        .await
        .with_context(|| {
            format!("Failed to fetch {name:?} protocol, in generalized form!")
        })?;

    task_set.add_handle(
        Id::PriceAlarms {
            protocol: name.clone(),
        },
        tokio::spawn(
            AlarmsGenerator::new_price_alarms(
                state.price_alarms.configuration.clone(),
                oracle
                    .check()
                    .await
                    .context("Failed to connect to oracle contract!")?
                    .0,
                state.price_alarms.gas_per_alarm,
                state.price_alarms.alarms_per_message,
                PriceAlarms { protocol: name },
            )
            .context("Failed to construct price alarms generator!")?
            .run(RunnableState::New),
        ),
    );

    Ok(state)
}

async fn remove_price_alarm_dispatcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    protocol: Arc<str>,
) -> Result<State> {
    task_set.abort(&Id::PriceAlarms { protocol });

    Ok(state)
}

#[inline]
fn error_handler(
    transaction_tx: unbounded::Sender<TxPackage<NoExpiration>>,
) -> impl AsyncFnMut(&mut TaskSet<Id, Result<()>>, State, Id) -> Result<State> + use<>
{
    async move |task_set: &mut TaskSet<Id, Result<()>>, mut state: State, id| {
        match id {
            Id::BalanceReporter {} => {
                spawn_restarting(task_set, state.balance_reporter.clone());
            },
            Id::Broadcaster {} => {
                spawn_restarting(task_set, state.broadcaster.clone());
            },
            Id::ProtocolWatcher {} => {
                spawn_restarting(task_set, state.protocol_watcher.clone());
            },
            Id::TimeAlarms {} => {
                spawn_restarting(task_set, state.time_alarms.clone());
            },
            Id::PriceAlarms { protocol: name } => {
                state = add_price_alarm_dispatcher(
                    task_set,
                    state,
                    name,
                    &transaction_tx,
                )
                .await?;
            },
        }

        Ok(state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Id {
    BalanceReporter,
    Broadcaster,
    ProtocolWatcher,
    TimeAlarms,
    PriceAlarms { protocol: Arc<str> },
}

impl Task<Id> for balance_reporter::State {
    const ID: Id = Id::BalanceReporter;
}

impl Task<Id> for broadcaster::State<NoExpiration> {
    const ID: Id = Id::Broadcaster;
}

impl Task<Id> for protocol_watcher::State {
    const ID: Id = Id::ProtocolWatcher;
}

impl Task<Id> for AlarmsGenerator<TimeAlarms> {
    const ID: Id = Id::TimeAlarms;
}

#[must_use]
pub struct PriceAlarmsState {
    configuration: Configuration,
    gas_per_alarm: Gas,
    alarms_per_message: u32,
}

#[must_use]
pub struct State {
    balance_reporter: balance_reporter::State,
    broadcaster: broadcaster::State<NoExpiration>,
    protocol_watcher: protocol_watcher::State,
    admin_contract: CheckedContract<Admin>,
    time_alarms: AlarmsGenerator<TimeAlarms>,
    price_alarms: PriceAlarmsState,
}

impl State {
    async fn new(
        mut service: Service,
        transaction_tx: unbounded::Sender<TxPackage<NoExpiration>>,
        transaction_rx: unbounded::Receiver<TxPackage<NoExpiration>>,
        action_tx: bounded::Sender<Command>,
        ApplicationDefinedContext {
            gas_per_time_alarm,
            time_alarms_per_message,
            gas_per_price_alarm,
            price_alarms_per_message,
        }: ApplicationDefinedContext,
    ) -> Result<State> {
        let Platform { time_alarms } = service
            .admin_contract
            .platform()
            .await
            .context("Failed to fetch platform contracts!")?;

        let (time_alarms, _) = time_alarms
            .check()
            .await
            .context("Failed to connect to time alarms contract!")?;

        let protocol_watcher = protocol_watcher::State {
            admin_contract: service.admin_contract.clone(),
            action_tx,
        };

        let configuration = Configuration {
            transaction_tx: transaction_tx.clone(),
            sender: service.signer.address().into(),
            idle_duration: service.idle_duration,
            query_tx: service.node_client.clone().query_tx(),
            timeout_duration: service.timeout_duration,
        };

        Ok(State {
            balance_reporter: balance_reporter(
                service.node_client.clone(),
                &service.signer,
            )?,
            broadcaster: broadcaster(
                service.node_client,
                service.signer,
                transaction_rx,
            )?,
            protocol_watcher,
            admin_contract: service.admin_contract,
            time_alarms: AlarmsGenerator::new_time_alarms(
                configuration.clone(),
                time_alarms,
                gas_per_time_alarm,
                time_alarms_per_message,
            )
            .context("Failed to construct time alarms generator!")?,
            price_alarms: PriceAlarmsState {
                configuration,
                gas_per_alarm: gas_per_price_alarm,
                alarms_per_message: price_alarms_per_message,
            },
        })
    }

    #[inline]
    pub const fn balance_reporter(&self) -> &balance_reporter::State {
        &self.balance_reporter
    }

    #[inline]
    pub const fn broadcaster(&self) -> &broadcaster::State<NoExpiration> {
        &self.broadcaster
    }

    #[inline]
    pub const fn protocol_watcher(&self) -> &protocol_watcher::State {
        &self.protocol_watcher
    }

    #[inline]
    pub const fn admin_contract(&self) -> &CheckedContract<Admin> {
        &self.admin_contract
    }

    #[inline]
    pub const fn time_alarms(&self) -> &AlarmsGenerator<TimeAlarms> {
        &self.time_alarms
    }

    #[inline]
    pub const fn price_alarms(&self) -> &PriceAlarmsState {
        &self.price_alarms
    }
}

fn balance_reporter(
    node_client: node::Client,
    signer: &Signer,
) -> Result<balance_reporter::State> {
    use balance_reporter::{Environment, State};

    Environment::read_from_env().map(|Environment { idle_duration }| State {
        query_bank: node_client.query_bank(),
        address: signer.address().into(),
        denom: signer.fee_token().into(),
        idle_duration,
    })
}

fn broadcaster(
    node_client: node::Client,
    signer: Signer,
    transaction_rx: unbounded::Receiver<TxPackage<NoExpiration>>,
) -> Result<broadcaster::State<NoExpiration>> {
    use broadcaster::{Environment, State};

    Environment::read_from_env().map(
        |Environment {
             delay_duration,
             retry_delay_duration,
         }| State {
            broadcast_tx: node_client.broadcast_tx(),
            signer: Arc::new(Mutex::new(signer)),
            transaction_rx: Arc::new(Mutex::new(transaction_rx)),
            delay_duration,
            retry_delay_duration,
        },
    )
}

pub struct ApplicationDefinedContext {
    pub gas_per_time_alarm: Gas,
    pub time_alarms_per_message: u32,
    pub gas_per_price_alarm: Gas,
    pub price_alarms_per_message: u32,
}

fn read_gas_per_time_alarm() -> Result<Gas> {
    Gas::read_from_var("TIME_ALARMS_GAS_LIMIT_PER_ALARM")
        .context("Failed to read gas limit per time alarm!")
}

fn read_time_alarms_per_message() -> Result<u32> {
    u32::read_from_var("TIME_ALARMS_MAX_ALARMS_GROUP")
        .context("Failed to read maximum count of time alarms per message!")
}

fn read_gas_per_price_alarm() -> Result<Gas> {
    Gas::read_from_var("PRICE_ALARMS_GAS_LIMIT_PER_ALARM")
        .context("Failed to read gas limit per price alarm!")
}

fn read_price_alarms_per_message() -> Result<u32> {
    u32::read_from_var("PRICE_ALARMS_MAX_ALARMS_GROUP")
        .context("Failed to read maximum count of price alarms per message!")
}
