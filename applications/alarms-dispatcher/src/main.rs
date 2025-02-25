use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use cosmrs::Gas;
use tokio::sync::Mutex;

use channel::{bounded, unbounded, Channel as _};
use contract::{
    Admin, CheckedContract, GeneralizedProtocol, GeneralizedProtocolContracts,
    Platform,
};
use environment::ReadFromVar as _;
use protocol_watcher::Command;
use service::{supervisor::configuration::Service, task::Runnable as _};
use supervisor::supervisor;
use ::task::RunnableState;
use task_set::TaskSet;
use tx::{NoExpiration, TxPackage};

use self::task::{AlarmsGenerator, Configuration, PriceAlarms, TimeAlarms};

mod task;

#[tokio::main]
async fn main() -> Result<()> {
    let service = Service::read_from_env().await?;

    let (tx, rx) = unbounded::Channel::new();

    supervisor::<_, _, bounded::Channel<_>, _, _, _>(
        init_tasks(
            service,
            tx.clone(),
            rx,
            ApplicationDefinedContext {
                gas_per_time_alarm: read_gas_per_time_alarm()?,
                time_alarms_per_message: read_time_alarms_per_message()?,
                gas_per_price_alarm: read_gas_per_price_alarm()?,
                price_alarms_per_message: read_price_alarms_per_message()?,
            },
        ),
        protocol_watcher::action_handler(
            tx.clone(),
            add_price_alarm_dispatcher,
            remove_price_alarm_dispatcher,
        ),
        error_handler(tx),
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

        task_set.add_handle(
            Id::BalanceReporter,
            tokio::spawn(
                state.balance_reporter.clone().run(RunnableState::New),
            ),
        );

        task_set.add_handle(
            Id::Broadcaster,
            tokio::spawn(state.broadcaster.clone().run(RunnableState::New)),
        );

        task_set.add_handle(
            Id::ProtocolWatcher,
            tokio::spawn(
                state.protocol_watcher.clone().run(RunnableState::New),
            ),
        );

        task_set.add_handle(
            Id::TimeAlarms,
            tokio::spawn(state.time_alarms.clone().run(RunnableState::New)),
        );

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
    } = state.admin_contract.generalized_protocol(&name).await?;

    task_set.add_handle(
        Id::PriceAlarms {
            protocol: name.clone(),
        },
        tokio::spawn(
            AlarmsGenerator::new_price_alarms(
                state.price_alarms.clone(),
                oracle.check().await?.0,
                PriceAlarms { protocol: name },
            )?
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
    _: unbounded::Sender<TxPackage<NoExpiration>>,
) -> impl AsyncFnMut(&mut TaskSet<Id, Result<()>>, State, Id) -> Result<State> + use<>
{
    async move |_, state, _| Ok(state)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Id {
    BalanceReporter,
    Broadcaster,
    ProtocolWatcher,
    TimeAlarms,
    PriceAlarms { protocol: Arc<str> },
}

struct State {
    balance_reporter: balance_reporter::State,
    broadcaster: broadcaster::State<NoExpiration>,
    protocol_watcher: protocol_watcher::State,
    admin_contract: CheckedContract<Admin>,
    time_alarms: AlarmsGenerator<TimeAlarms>,
    price_alarms: Configuration,
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
        let balance_reporter = balance_reporter::State {
            query_bank: service.node_client.clone().query_bank(),
            address: service.signer.address().into(),
            denom: service.signer.fee_token().into(),
            idle_duration: Duration::default(),
        };

        let broadcaster = broadcaster::State {
            broadcast_tx: service.node_client.clone().broadcast_tx(),
            signer: Arc::new(Mutex::new(service.signer)),
            transaction_rx: Arc::new(Mutex::new(transaction_rx)),
            delay_duration: service.broadcast_delay_duration,
            retry_delay_duration: service.broadcast_retry_delay_duration,
        };

        let Platform { time_alarms } =
            service.admin_contract.platform().await?;

        let (time_alarms, _) = time_alarms.check().await?;

        let protocol_watcher = protocol_watcher::State {
            admin_contract: service.admin_contract.clone(),
            action_tx,
        };

        Ok(State {
            balance_reporter,
            broadcaster,
            protocol_watcher,
            admin_contract: service.admin_contract,
            time_alarms: AlarmsGenerator::new_time_alarms(
                Configuration {
                    transaction_tx: transaction_tx.clone(),
                    sender: String::new(),
                    alarms_per_message: time_alarms_per_message,
                    gas_per_alarm: gas_per_time_alarm,
                    idle_duration: service.idle_duration,
                    query_tx: service.node_client.clone().query_tx(),
                    timeout_duration: service.timeout_duration,
                }
                .clone(),
                time_alarms,
                TimeAlarms {},
            )?,
            price_alarms: Configuration {
                transaction_tx,
                sender: String::new(),
                alarms_per_message: price_alarms_per_message,
                gas_per_alarm: gas_per_price_alarm,
                idle_duration: service.idle_duration,
                query_tx: service.node_client.query_tx(),
                timeout_duration: service.timeout_duration,
            },
        })
    }
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
