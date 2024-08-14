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

use crate::service::task_spawner::{
    CancellationToken, ServiceStopped, TaskSpawner,
};

use self::{
    balance_reporter::BalanceReporter, broadcast::Broadcast,
    protocol_watcher::ProtocolWatcher,
};

pub mod application_defined;
pub mod balance_reporter;
pub mod broadcast;
pub mod protocol_watcher;

pub trait Runnable: Sized {
    fn run(self) -> impl Future<Output = Result<()>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Id<T>
where
    T: application_defined::Id,
{
    BalanceReporter,
    Broadcast,
    ProtocolWatcher,
    ApplicationDefined(T),
}

impl<T> Id<T>
where
    T: application_defined::Id,
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

pub enum Task<T>
where
    T: application_defined::Task,
{
    BalanceReporter(BalanceReporter),
    Broadcast(Broadcast<T::TxExpiration>),
    ProtocolWatcher(ProtocolWatcher),
    ApplicationDefined(T),
}

impl<T> Task<T>
where
    T: application_defined::Task,
{
    pub async fn run(
        self,
        task_spawner: &TaskSpawner<Id<T::Id>, Result<()>>,
        task_states: &mut BTreeMap<Id<T::Id>, State>,
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

    fn identifier(&self) -> Id<T::Id> {
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
pub struct WithExpiration {
    expires_at: Instant,
}

impl WithExpiration {
    pub const fn new(expires_at: Instant) -> Self {
        Self { expires_at }
    }
}

impl TxExpiration for WithExpiration {
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
