use std::{
    collections::{btree_map::Entry as BTreeMapEntry, BTreeMap, VecDeque},
    convert::identity,
    future::pending,
    marker::PhantomData,
    time::Duration,
};

use anyhow::{Context as _, Result};
use tokio::{
    select,
    time::{sleep_until, Instant},
};

use channel::Channel as _;

use crate::{
    service::{task_spawner::TaskSpawner, TaskResult, TaskResultsReceiver},
    task::{
        self,
        application_defined::{self, Id as _},
        protocol_watcher::Command as ProtocolWatcherCommand,
        BalanceReporter, Broadcast, ProtocolWatcher, State as TaskState, Task,
        TxPackage,
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
pub struct Supervisor<
    BalanceReporter,
    Broadcast,
    ProtocolWatcher,
    ApplicationDefined,
> where
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
    configuration: Configuration<ApplicationDefined::Id>,
    task_spawner: TaskSpawner<task::Id<ApplicationDefined::Id>, Result<()>>,
    task_result_rx:
        TaskResultsReceiver<task::Id<ApplicationDefined::Id>, Result<()>>,
    task_states: BTreeMap<task::Id<ApplicationDefined::Id>, TaskState>,
    restart_queue: VecDeque<(Instant, task::Id<ApplicationDefined::Id>)>,
    transaction_tx:
        channel::unbounded::Sender<TxPackage<ApplicationDefined::TxExpiration>>,
    protocol_watcher_rx: channel::bounded::Receiver<ProtocolWatcherCommand>,
    _balance_reporter: PhantomData<BalanceReporter>,
    _broadcast: PhantomData<Broadcast>,
    _protocol_watcher: PhantomData<ProtocolWatcher>,
}

impl<BalanceReporter, Broadcast, ProtocolWatcher, ApplicationDefined>
    Supervisor<BalanceReporter, Broadcast, ProtocolWatcher, ApplicationDefined>
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
    pub async fn new<U>(
        configuration: Configuration<ApplicationDefined::Id>,
        task_spawner: TaskSpawner<task::Id<ApplicationDefined::Id>, Result<()>>,
        task_result_rx: TaskResultsReceiver<
            task::Id<ApplicationDefined::Id>,
            Result<()>,
        >,
        application: &'static str,
        version: &'static str,
        tasks: U,
    ) -> Result<Self>
    where
        U: IntoIterator<Item = ApplicationDefined::Id>,
    {
        log!(info!(
            %application,
            %version,
            "Starting up supervisor.",
        ));

        let (transaction_tx, transaction_rx) =
            channel::unbounded::Channel::new();

        let (protocol_watcher_tx, protocol_watcher_rx) =
            channel::bounded::Channel::new();

        let mut supervisor = Self {
            configuration,
            task_spawner,
            task_result_rx,
            task_states: BTreeMap::new(),
            restart_queue: VecDeque::new(),
            transaction_tx,
            protocol_watcher_rx,
            _balance_reporter: PhantomData,
            _broadcast: PhantomData,
            _protocol_watcher: PhantomData,
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
        const TASK_RESULTS_CHANNEL_CLOSED_ERROR: &str =
            "Task results channel closed unexpectedly!";

        log!(info!("Running."));

        loop {
            select!(
                biased;
                task_result = self.task_result_rx.recv() => {
                    let result =
                        task_result.context(TASK_RESULTS_CHANNEL_CLOSED_ERROR)?;

                    self.handle_task_result_and_restart(result).await
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
            )
            .inspect_err(|error| {
                log!(error!(?error, "Fatal error occurred!"));
            })?;
        }
    }

    async fn start_tasks<U>(
        &mut self,
        transaction_rx: channel::unbounded::Receiver<
            TxPackage<ApplicationDefined::TxExpiration>,
        >,
        protocol_watcher_tx: channel::bounded::Sender<ProtocolWatcherCommand>,
        tasks: U,
    ) -> Result<()>
    where
        U: IntoIterator<Item = ApplicationDefined::Id>,
    {
        Task::<
            BalanceReporter,
            Broadcast,
            ProtocolWatcher,
            ApplicationDefined,
        >::BalanceReporter(self.create_balance_reporter_task())
            .run(&self.task_spawner, &mut self.task_states)
            .await
            .context("Failed to start balance reporter task!")?;

        Task::<
            BalanceReporter,
            Broadcast,
            ProtocolWatcher,
            ApplicationDefined,
        >::Broadcast(self.create_broadcast_task_with(transaction_rx))
            .run(&self.task_spawner, &mut self.task_states)
            .await
            .context("Failed to start broadcaster task!")?;

        Task::<
            BalanceReporter,
            Broadcast,
            ProtocolWatcher,
            ApplicationDefined,
        >::ProtocolWatcher(
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

    async fn handle_task_result_and_restart(
        &mut self,
        task_result: TaskResult<task::Id<ApplicationDefined::Id>, Result<()>>,
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

    async fn run_task(
        &mut self,
        task_id: task::Id<ApplicationDefined::Id>,
    ) -> Result<()> {
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
                .into_task(
                    &mut self.configuration.service_configuration,
                    &mut self.configuration.task_creation_context,
                    &self.transaction_tx,
                )
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
    fn create_balance_reporter_task(&self) -> BalanceReporter {
        BalanceReporter::new(&self.configuration.service_configuration)
    }

    #[inline]
    fn create_broadcast_task(&mut self) -> Broadcast {
        let transaction_rx;

        (self.transaction_tx, transaction_rx) =
            channel::unbounded::Channel::new();

        self.create_broadcast_task_with(transaction_rx)
    }

    fn create_broadcast_task_with(
        &self,
        transaction_rx: channel::unbounded::Receiver<
            TxPackage<Broadcast::TxExpiration>,
        >,
    ) -> Broadcast {
        Broadcast::new(
            &self.configuration.service_configuration,
            transaction_rx,
        )
    }

    #[inline]
    fn create_protocol_watcher_task(&mut self) -> ProtocolWatcher {
        let protocol_watcher_tx;

        (protocol_watcher_tx, self.protocol_watcher_rx) =
            channel::bounded::Channel::new();

        self.create_protocol_watcher_task_with(protocol_watcher_tx)
    }

    fn create_protocol_watcher_task_with(
        &self,
        protocol_watcher_tx: channel::bounded::Sender<ProtocolWatcherCommand>,
    ) -> ProtocolWatcher {
        ProtocolWatcher::new(
            &self.configuration.service_configuration,
            &self.task_states,
            protocol_watcher_tx,
        )
    }

    fn place_on_restart_queue(
        &mut self,
        task_id: task::Id<ApplicationDefined::Id>,
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
        task_result: TaskResult<task::Id<ApplicationDefined::Id>, Result<()>>,
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

                self.cancel_tasks().await.context("Killing tasks failed!")?;
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

        let tasks = 0..{
            let tasks_count = self.task_states.len();

            self.task_states.clear();

            tasks_count
        };

        for _ in tasks {
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

        log!(info!("Killed worker tasks."));

        Ok(())
    }

    async fn handle_protocol_command(
        &mut self,
        protocol_command: ProtocolWatcherCommand,
    ) -> Result<()> {
        match protocol_command {
            ProtocolWatcherCommand::ProtocolAdded(protocol) => {
                for id in ApplicationDefined::protocol_task_set_ids(protocol) {
                    self.run_task(task::Id::ApplicationDefined(id)).await?;
                }
            },
            ProtocolWatcherCommand::ProtocolRemoved(ref protocol) => {
                () = self.task_states.retain(|id, _| match id {
                    task::Id::ApplicationDefined(id) => {
                        id.protocol() != Some(protocol)
                    },
                    _ => true,
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

#[cold]
#[inline]
fn cold() {}
