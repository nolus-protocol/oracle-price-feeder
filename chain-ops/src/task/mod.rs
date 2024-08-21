use std::{
    borrow::Cow,
    collections::{btree_map::Entry as BTreeMapEntry, BTreeMap},
    convert::Infallible,
    error::Error,
    future::Future,
    sync::Arc,
};

use anyhow::Result;
use cosmrs::{
    proto::cosmos::base::abci::v1beta1::TxResponse, tx::Body as TxBody, Gas,
};
use tokio::{
    sync::oneshot,
    time::{error::Elapsed, timeout_at, Instant},
};
use tracing::{error, error_span};

use crate::{
    channel,
    service::task_spawner::{CancellationToken, ServiceStopped, TaskSpawner},
};

pub mod application_defined;
pub mod balance_reporter;
pub mod broadcast;
pub mod protocol_watcher;

pub trait Runnable: Sized {
    fn run(self) -> impl Future<Output = Result<()>> + Send;
}

pub trait BuiltIn: Runnable + Send + Sized + 'static {
    type ServiceConfiguration;
}

pub trait BalanceReporter: BuiltIn {
    fn new(service_configuration: &Self::ServiceConfiguration) -> Self;
}

pub trait Broadcast: BuiltIn {
    type TxExpiration: TxExpiration;

    fn new(
        service_configuration: &Self::ServiceConfiguration,
        transaction_rx: channel::unbounded::Receiver<
            TxPackage<Self::TxExpiration>,
        >,
    ) -> Self;
}

pub trait ProtocolWatcher: BuiltIn {
    fn new<ApplicationDefined>(
        service_configuration: &Self::ServiceConfiguration,
        task_states: &BTreeMap<Id<ApplicationDefined>, State>,
        command_tx: channel::bounded::Sender<protocol_watcher::Command>,
    ) -> Self
    where
        ApplicationDefined: application_defined::Id;
}

pub enum Task<BalanceReporter, Broadcast, ProtocolWatcher, ApplicationDefined>
where
    BalanceReporter: self::BalanceReporter,
    Broadcast: self::Broadcast<
        ServiceConfiguration = BalanceReporter::ServiceConfiguration,
    >,
    ProtocolWatcher: self::ProtocolWatcher<
        ServiceConfiguration = BalanceReporter::ServiceConfiguration,
    >,
    ApplicationDefined: application_defined::Task<
        TxExpiration = Broadcast::TxExpiration,
        Id: application_defined::Id<
            ServiceConfiguration = BalanceReporter::ServiceConfiguration,
        >,
    >,
{
    BalanceReporter(BalanceReporter),
    Broadcast(Broadcast),
    ProtocolWatcher(ProtocolWatcher),
    ApplicationDefined(ApplicationDefined),
}

impl<BalanceReporter, Broadcast, ProtocolWatcher, ApplicationDefined>
    Task<BalanceReporter, Broadcast, ProtocolWatcher, ApplicationDefined>
where
    BalanceReporter: self::BalanceReporter,
    Broadcast: self::Broadcast<
        ServiceConfiguration = BalanceReporter::ServiceConfiguration,
    >,
    ProtocolWatcher: self::ProtocolWatcher<
        ServiceConfiguration = BalanceReporter::ServiceConfiguration,
    >,
    ApplicationDefined: application_defined::Task<
        TxExpiration = Broadcast::TxExpiration,
        Id: application_defined::Id<
            ServiceConfiguration = BalanceReporter::ServiceConfiguration,
        >,
    >,
{
    pub async fn run(
        self,
        task_spawner: &TaskSpawner<Id<ApplicationDefined::Id>, Result<()>>,
        task_states: &mut BTreeMap<Id<ApplicationDefined::Id>, State>,
    ) -> Result<(), ServiceStopped> {
        let task_id = self.identifier();

        match self {
            Self::BalanceReporter(task) => {
                task_spawner
                    .spawn(task_id.clone(), run(task_id.clone(), task))
                    .await
            },
            Self::Broadcast(task) => {
                task_spawner
                    .spawn(task_id.clone(), run(task_id.clone(), task))
                    .await
            },
            Self::ProtocolWatcher(task) => {
                task_spawner
                    .spawn(task_id.clone(), run(task_id.clone(), task))
                    .await
            },
            Self::ApplicationDefined(task) => {
                task_spawner
                    .spawn(task_id.clone(), run(task_id.clone(), task))
                    .await
            },
        }
        .map(|cancellation_token| match task_states.entry(task_id) {
            BTreeMapEntry::Vacant(entry) => {
                entry.insert(State::new(cancellation_token));
            },
            BTreeMapEntry::Occupied(entry) => {
                entry.into_mut().replace_and_increment(cancellation_token);
            },
        })
    }

    fn identifier(&self) -> Id<ApplicationDefined::Id> {
        match self {
            Self::BalanceReporter { .. } => Id::BalanceReporter,
            Self::Broadcast { .. } => Id::Broadcast,
            Self::ProtocolWatcher { .. } => Id::ProtocolWatcher,
            Self::ApplicationDefined { 0: task } => {
                Id::ApplicationDefined(task.id())
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Id<ApplicationDefined>
where
    ApplicationDefined: application_defined::Id,
{
    BalanceReporter,
    Broadcast,
    ProtocolWatcher,
    ApplicationDefined(ApplicationDefined),
}

impl<ApplicationDefined> Id<ApplicationDefined>
where
    ApplicationDefined: application_defined::Id,
{
    pub fn name(&self) -> Cow<'static, str> {
        match &self {
            Self::BalanceReporter => Cow::Borrowed("Balance Reporter"),
            Self::Broadcast => Cow::Borrowed("Broadcast"),
            Self::ProtocolWatcher => Cow::Borrowed("Protocol Watcher"),
            Self::ApplicationDefined(id) => id.name(),
        }
    }
}

pub struct TxPackage<Expiration>
where
    Expiration: TxExpiration,
{
    pub tx_body: TxBody,
    pub source: Arc<str>,
    pub hard_gas_limit: Gas,
    pub fallback_gas: Gas,
    pub feedback_sender: oneshot::Sender<TxResponse>,
    pub expiration: Expiration,
}

pub trait TxExpiration: Copy + Send + Sized + 'static {
    type Expired: Error + 'static;

    fn with_expiration<F>(
        self,
        future: F,
    ) -> impl Future<Output = Result<F::Output, Self::Expired>> + Send
    where
        F: Future + Send;
}

#[derive(Clone, Copy)]
#[must_use]
pub struct NoExpiration;

impl TxExpiration for NoExpiration {
    type Expired = Infallible;

    #[inline]
    async fn with_expiration<F>(
        self,
        future: F,
    ) -> Result<F::Output, Self::Expired>
    where
        F: Future + Send,
    {
        Ok(future.await)
    }
}

#[derive(Clone, Copy)]
#[must_use]
pub struct TimeBasedExpiration {
    expires_at: Instant,
}

impl TimeBasedExpiration {
    pub const fn new(expires_at: Instant) -> Self {
        Self { expires_at }
    }
}

impl TxExpiration for TimeBasedExpiration {
    type Expired = Elapsed;

    #[inline]
    fn with_expiration<F>(
        self,
        future: F,
    ) -> impl Future<Output = Result<F::Output, Self::Expired>> + Send
    where
        F: Future + Send,
    {
        timeout_at(self.expires_at, future)
    }
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

async fn run<Id, T>(id: self::Id<Id>, runnable: T) -> Result<()>
where
    Id: application_defined::Id,
    T: Runnable,
{
    runnable.run().await.inspect_err(|error| {
        error_span!("run").in_scope(|| {
            error!(
                target: "task",
                ?error,
                "{} task exited with an error!",
                id.name(),
            );
        });
    })
}
