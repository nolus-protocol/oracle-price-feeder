use std::{
    collections::{btree_map::Entry as BTreeMapEntry, BTreeMap, VecDeque},
    convert::identity,
    future::pending,
    time::Duration,
};

use anyhow::{Context as _, Result};
use tokio::{
    select,
    time::{sleep_until, Instant},
};

use crate::{
    channel::{bounded, unbounded, Channel as _},
    contract::Admin as AdminContract,
    node,
    service::{task_spawner::TaskSpawner, TaskResult, TaskResultsReceiver},
    signer::Signer,
    task::{
        self,
        application_defined::{self, Id as _},
        balance_reporter::BalanceReporter,
        broadcast::Broadcast,
        protocol_watcher::{
            Command as ProtocolWatcherCommand, ProtocolWatcher,
        },
        State as TaskState, Task, TxPackage,
    },
};

use self::configuration::Configuration;

pub mod configuration;

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "supervisor",
            $($body)+
        )
    };
}

pub mod log;

#[must_use]
pub struct Supervisor<T>
where
    T: application_defined::Task,
{
    node_client: node::Client,
    signer: Signer,
    admin_contract: AdminContract,
    task_spawner: TaskSpawner<task::Id<T::Id>, Result<()>>,
    task_result_rx: TaskResultsReceiver<task::Id<T::Id>, Result<()>>,
    task_states: BTreeMap<task::Id<T::Id>, TaskState>,
    restart_queue: VecDeque<(Instant, task::Id<T::Id>)>,
    transaction_tx: unbounded::Sender<TxPackage<T::TxExpiration>>,
    protocol_watcher_rx: bounded::Receiver<ProtocolWatcherCommand>,
    idle_duration: Duration,
    timeout_duration: Duration,
    broadcast_delay_duration: Duration,
    broadcast_retry_delay_duration: Duration,
    task_creation_context: application_defined::TaskCreationContext<T>,
}

impl<T> Supervisor<T>
where
    T: application_defined::Task,
{
    pub async fn new<U>(
        Configuration {
            node_client,
            signer,
            admin_contract_address,
            task_spawner,
            task_result_rx,
            idle_duration,
            timeout_duration,
            broadcast_delay_duration,
            broadcast_retry_delay_duration,
            task_creation_context,
        }: Configuration<T>,
        version: &'static str,
        tasks: U,
    ) -> Result<Self>
    where
        U: Iterator<Item = T::Id>,
    {
        log!(info!(
            %version,
            sender_address = %signer.address(),
            "Starting up supervisor.",
        ));

        let (transaction_tx, transaction_rx) = unbounded::Channel::new();

        let (protocol_watcher_tx, protocol_watcher_rx) =
            bounded::Channel::new();

        let mut supervisor = Self {
            node_client: node_client.clone(),
            signer,
            admin_contract: AdminContract::new(
                node_client.query_wasm(),
                admin_contract_address.clone(),
            ),
            task_spawner,
            task_result_rx,
            task_states: BTreeMap::new(),
            restart_queue: VecDeque::new(),
            transaction_tx,
            protocol_watcher_rx,
            idle_duration,
            timeout_duration,
            broadcast_delay_duration,
            broadcast_retry_delay_duration,
            task_creation_context,
        };

        log!(info!("Starting worker tasks."));

        supervisor
            .start_tasks(transaction_rx, protocol_watcher_tx, tasks)
            .await
            .inspect(|()| log!(info!("Worker tasks started.")))
            .map(|()| supervisor)
            .context("Failed to start initial tasks!")
    }

    #[inline]
    pub async fn run(mut self) -> Result<()> {
        log!(info!("Running."));

        loop {
            let result = select! {
                biased;
                task_result = self.task_result_rx.recv() => {
                    self.handle_task_result_and_restart(
                        Self::handle_task_result_channel_output(task_result)?,
                    )
                    .await
                },
                Some(protocol_command) = self.protocol_watcher_rx.recv() => {
                    self.handle_protocol_command(protocol_command)
                        .await
                        .context("Failed to handle protocol command!")
                },
                task_id = Self::next_restart_task_future(
                    &mut self.restart_queue,
                ), if !self.restart_queue.is_empty() => {
                    self.run_task(task_id).await
                },
            };

            match result {
                Ok(()) => {},
                Err(error) => {
                    log!(error!(?error, "Fatal error occurred!"));

                    break Err(error);
                },
            }
        }
    }

    async fn start_tasks<U>(
        &mut self,
        transaction_rx: unbounded::Receiver<TxPackage<T::TxExpiration>>,
        protocol_watcher_tx: bounded::Sender<ProtocolWatcherCommand>,
        tasks: U,
    ) -> Result<()>
    where
        U: Iterator<Item = T::Id>,
    {
        Task::<T>::BalanceReporter(self.create_balance_reporter_task())
            .run(&self.task_spawner, &mut self.task_states)
            .await
            .context("Failed to start balance reporter task!")?;

        Task::<T>::Broadcast(self.create_broadcast_task_with(transaction_rx))
            .run(&self.task_spawner, &mut self.task_states)
            .await
            .context("Failed to start broadcaster task!")?;

        Task::<T>::ProtocolWatcher(
            self.create_protocol_watcher_task_with(protocol_watcher_tx),
        )
        .run(&self.task_spawner, &mut self.task_states)
        .await
        .context("Failed to start protocol watcher task!")?;

        for task_id in tasks {
            self.run_task(task::Id::ApplicationDefined(task_id))
                .await
                .context("Failed to start application-defined task!")?;
        }

        Ok(())
    }

    fn handle_task_result_channel_output(
        task_result: Option<TaskResult<task::Id<T::Id>, Result<()>>>,
    ) -> Result<TaskResult<task::Id<T::Id>, Result<()>>> {
        const TASK_RESULTS_CHANNEL_CLOSED_ERROR: &str =
            "Task results channel closed unexpectedly!";

        task_result.context(TASK_RESULTS_CHANNEL_CLOSED_ERROR)
    }

    async fn handle_task_result_and_restart(
        &mut self,
        task_result: TaskResult<task::Id<T::Id>, Result<()>>,
    ) -> Result<()> {
        const MAX_CONSEQUENT_RETRIES: u8 = 2;

        let task_id = task_result.identifier.clone();

        () = self
            .handle_task_result(task_result)
            .await
            .context("Failed to handle exited task's result!")?;

        if let BTreeMapEntry::Occupied(mut entry) =
            self.task_states.entry(task_id)
        {
            if entry.get_mut().retry() >= MAX_CONSEQUENT_RETRIES {
                let task_id = entry.remove_entry().0;

                self.place_on_restart_queue(task_id)
                    .context("Failed to put task on restart queue!")
            } else {
                let task_id = entry.key().clone();

                self.run_task(task_id)
                    .await
                    .context("Failed to restart task!")
            }
        } else {
            Ok(())
        }
    }

    async fn run_task(&mut self, task_id: task::Id<T::Id>) -> Result<()> {
        let result = match task_id.clone() {
            task::Id::BalanceReporter => {
                Ok(Task::BalanceReporter(self.create_balance_reporter_task()))
            },
            task::Id::Broadcast => {
                Ok(Task::Broadcast(self.create_broadcast_task()))
            },
            task::Id::ProtocolWatcher => {
                Ok(Task::ProtocolWatcher(self.create_protocol_watcher_task()))
            },
            task::Id::ApplicationDefined(id) => id
                .into_task(TaskCreationContext {
                    node_client: &mut self.node_client,
                    signer_address: self.signer.address(),
                    admin_contract: &mut self.admin_contract,
                    transaction_tx: &self.transaction_tx,
                    idle_duration: self.idle_duration,
                    timeout_duration: self.timeout_duration,
                    application_defined: &mut self.task_creation_context,
                })
                .await
                .map(Task::ApplicationDefined),
        };

        match result {
            Ok(task) => task
                .run(&self.task_spawner, &mut self.task_states)
                .await
                .map_err(Into::into),
            Err(error) => {
                log!(error!(
                    task = %task_id.name(),
                    ?error,
                    "Failed to create task! Placing on restart queue.",
                ));

                self.place_on_restart_queue(task_id)
            },
        }
    }

    #[inline]
    fn create_balance_reporter_task(&mut self) -> BalanceReporter {
        BalanceReporter::new(
            self.node_client.clone().query_bank(),
            self.signer.address().into(),
            self.signer.fee_token().into(),
        )
    }

    #[inline]
    fn create_broadcast_task(&mut self) -> Broadcast<T::TxExpiration> {
        let transaction_rx;

        (self.transaction_tx, transaction_rx) = unbounded::Channel::new();

        self.create_broadcast_task_with(transaction_rx)
    }

    fn create_broadcast_task_with(
        &self,
        transaction_rx: unbounded::Receiver<TxPackage<T::TxExpiration>>,
    ) -> Broadcast<T::TxExpiration> {
        Broadcast::new(
            self.node_client.clone().broadcast_tx(),
            self.signer.clone(),
            transaction_rx,
            self.broadcast_delay_duration,
            self.broadcast_retry_delay_duration,
        )
    }

    #[inline]
    fn create_protocol_watcher_task(&mut self) -> ProtocolWatcher {
        let protocol_watcher_tx;

        (protocol_watcher_tx, self.protocol_watcher_rx) =
            bounded::Channel::new();

        self.create_protocol_watcher_task_with(protocol_watcher_tx)
    }

    fn create_protocol_watcher_task_with(
        &self,
        protocol_watcher_tx: bounded::Sender<ProtocolWatcherCommand>,
    ) -> ProtocolWatcher {
        ProtocolWatcher::new(
            self.admin_contract.clone(),
            self.task_states
                .keys()
                .filter_map(|id| {
                    if let task::Id::ApplicationDefined(id) = id {
                        id.protocol().cloned()
                    } else {
                        None
                    }
                })
                .collect(),
            protocol_watcher_tx,
        )
    }

    fn place_on_restart_queue(
        &mut self,
        task_id: task::Id<T::Id>,
    ) -> Result<()> {
        log!(warn!(
            task = %task_id.name(),
            "Placing task in deferred restart queue.",
        ));

        Instant::now()
            .checked_add(
                if matches!(task_id, task::Id::ApplicationDefined { .. }) {
                    const { Duration::from_secs(180) }
                } else {
                    const { Duration::from_secs(10) }
                },
            )
            .map(|instant| {
                () = self.restart_queue.push_back((instant, task_id));
            })
            .context("Failed to calculate task restart timestamp!")
    }

    async fn handle_task_result(
        &mut self,
        task_result: TaskResult<task::Id<T::Id>, Result<()>>,
    ) -> Result<()> {
        match task_result {
            TaskResult {
                identifier: task::Id::BalanceReporter,
                result,
            } => log::balance_reporter_result(result),
            TaskResult {
                identifier: task::Id::Broadcast,
                result,
            } => {
                log::broadcast_result(result);

                self.cancel_tasks().await?;
            },
            TaskResult {
                identifier: task::Id::ProtocolWatcher,
                result,
            } => log::protocol_watcher_result(result),
            TaskResult {
                identifier: task::Id::ApplicationDefined(id),
                result,
            } => log::application_defined_result(&id, result),
        }

        Ok(())
    }

    async fn cancel_tasks(&mut self) -> Result<()> {
        log!(info!("Killing worker tasks."));

        let tasks_count = self.task_states.len();

        self.task_states.clear();

        for _ in 0..tasks_count {
            match self.task_result_rx.recv().await {
                Some(TaskResult {
                    identifier: task::Id::BalanceReporter,
                    result,
                }) => log::balance_reporter_result(result),
                Some(TaskResult {
                    identifier: task::Id::Broadcast,
                    result,
                }) => {
                    cold();

                    return result
                        .map_err(Into::into)
                        .and_then(identity)
                        .context(
                            "Broadcast task shouldn't be reported back a second time!",
                        );
                },
                Some(TaskResult {
                    identifier: task::Id::ProtocolWatcher,
                    result,
                }) => log::protocol_watcher_result(result),
                Some(TaskResult {
                    identifier: task::Id::ApplicationDefined(id),
                    result,
                }) => log::application_defined_result(&id, result),
                None => panic!("Task results channel closed unexpectedly!"),
            }
        }

        assert!(self.task_states.is_empty());

        Ok(())
    }

    async fn handle_protocol_command(
        &mut self,
        protocol_command: ProtocolWatcherCommand,
    ) -> Result<()> {
        match protocol_command {
            ProtocolWatcherCommand::ProtocolAdded(protocol) => {
                for id in T::protocol_task_set_ids(protocol) {
                    self.run_task(task::Id::ApplicationDefined(id)).await?;
                }
            },
            ProtocolWatcherCommand::ProtocolRemoved(ref protocol) => {
                () = self.task_states.retain(|id, _| match id {
                    task::Id::ApplicationDefined(id) => {
                        id.protocol().map_or(false, |task_protocol| {
                            task_protocol == protocol
                        })
                    },
                    _ => false,
                });
            },
        }

        Ok(())
    }

    async fn next_restart_task_future<U>(
        restart_queue: &mut VecDeque<(Instant, task::Id<U>)>,
    ) -> task::Id<U>
    where
        U: application_defined::Id,
    {
        if let Some(&(instant, _)) = restart_queue.front() {
            sleep_until(instant).await;

            if let Some((_, id)) = restart_queue.pop_front() {
                id
            } else {
                unreachable!(
                    "Restart queue cannot be empty as it's behind a mutable \
                    reference and already known to have at least one element!",
                )
            }
        } else {
            pending().await
        }
    }
}

#[must_use]
pub struct TaskCreationContext<'r, T>
where
    T: application_defined::Task,
{
    node_client: &'r mut node::Client,
    signer_address: &'r str,
    admin_contract: &'r mut AdminContract,
    transaction_tx: &'r unbounded::Sender<TxPackage<T::TxExpiration>>,
    idle_duration: Duration,
    timeout_duration: Duration,
    application_defined: &'r mut application_defined::TaskCreationContext<T>,
}

impl<'r, T> TaskCreationContext<'r, T>
where
    T: application_defined::Task,
{
    #[must_use]
    pub fn node_client(&mut self) -> &mut node::Client {
        self.node_client
    }

    #[must_use]
    pub const fn signer_address(&self) -> &str {
        self.signer_address
    }

    pub fn admin_contract(&mut self) -> &mut AdminContract {
        self.admin_contract
    }

    #[must_use]
    pub const fn transaction_tx(
        &self,
    ) -> &unbounded::Sender<TxPackage<T::TxExpiration>> {
        self.transaction_tx
    }

    #[must_use]
    pub const fn idle_duration(&self) -> Duration {
        self.idle_duration
    }

    #[must_use]
    pub const fn timeout_duration(&self) -> Duration {
        self.timeout_duration
    }

    #[must_use]
    pub fn application_defined(
        &mut self,
    ) -> &mut application_defined::TaskCreationContext<T> {
        self.application_defined
    }
}

#[cold]
#[inline]
fn cold() {}
