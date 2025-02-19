use std::{convert::identity, path::Path};

use anyhow::{Context as _, Result};

use crate::{
    service::{self, ShutdownResult},
    supervisor::{
        self,
        configuration::{self, Configuration},
    },
    task::{
        application_defined, balance_reporter::BalanceReporter,
        broadcast::Broadcast, protocol_watcher::ProtocolWatcher,
    },
};

#[inline]
pub async fn run<
    LogsDirectory,
    TaskCreationContextCtor,
    StartupTasksFunctor,
    StartupTasksIter,
>(
    application_name: &'static str,
    application_version: &'static str,
    logs_directory: LogsDirectory,
    task_creation_context: TaskCreationContextCtor,
    startup_tasks: StartupTasksFunctor,
) -> Result<()>
where
    LogsDirectory: AsRef<Path>,
    TaskCreationContextCtor: FnOnce() -> Result<
        <StartupTasksIter::Item as application_defined::Id>::TaskCreationContext,
    >,
    StartupTasksFunctor: FnOnce() -> StartupTasksIter,
    StartupTasksIter: IntoIterator + Send + 'static,
    StartupTasksIter::IntoIter: Send,
    StartupTasksIter::Item: application_defined::Id<ServiceConfiguration=configuration::Service> + Unpin,
{
    log::init(logs_directory).context("Failed to initialize logging!")?;

    let service_configuration =
        configuration::Service::read_from_env()
            .await
            .context("Failed to read service configuration!")?;

    let task_creation_context = task_creation_context()
        .context("Failed to construct task creation context!")?;

    service::run({
        let startup_tasks = startup_tasks();

        move |task_spawner, task_result_rx| async move {
            Supervisor::<StartupTasksIter::Item>::new(
                Configuration::new(
                    service_configuration,
                    task_creation_context,
                ),
                task_spawner,
                task_result_rx,
                application_name,
                application_version,
                startup_tasks,
            )
            .await
            .context("Failed to create tasks supervisor!")?
            .run()
            .await
            .context("Supervisor exited with an error!")
        }
    })
    .await
    .context("Running service failed!")
    .and_then(|result| match result {
        ShutdownResult::Exited(result) => result
            .context("Failed to join task back to it's parent!")
            .and_then(identity),
        ShutdownResult::StopSignalReceived => Ok(()),
    })
}

type Supervisor<Id> = supervisor::Supervisor<
    BalanceReporter,
    Broadcast<TxExpiration<Id>>,
    ProtocolWatcher,
    Task<Id>,
>;

type TxExpiration<Id> = <Task<Id> as application_defined::Task>::TxExpiration;

type Task<Id> = <Id as application_defined::Id>::Task;
