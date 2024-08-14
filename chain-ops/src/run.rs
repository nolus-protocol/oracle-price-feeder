use std::{convert::identity, path::Path};

use anyhow::{Context as _, Result};

use crate::{
    log,
    service::{self, ShutdownResult},
    supervisor::{
        configuration::{self, Configuration},
        Supervisor,
    },
    task::application_defined::{Id, TaskCreationContext},
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
        TaskCreationContext<<StartupTasksIter::Item as Id>::Task>,
    >,
    StartupTasksFunctor: FnOnce() -> StartupTasksIter,
    StartupTasksIter: Iterator + Send + 'static,
    StartupTasksIter::Item: Id + Unpin,
{
    log::init(logs_directory).context("Failed to initialize logging!")?;

    let configuration = configuration::Static::read_from_env()
        .await
        .context("Failed to read service configuration!")?;

    let task_creation_context = task_creation_context()
        .context("Failed to construct task creation context!")?;

    let startup_tasks = startup_tasks();

    service::run(move |task_spawner, task_result_rx| async move {
        Supervisor::new(
            Configuration::<<StartupTasksIter::Item as Id>::Task>::new(
                configuration,
                task_spawner,
                task_result_rx,
                task_creation_context,
            ),
            application_name,
            application_version,
            startup_tasks,
        )
        .await
        .context("Failed to create tasks supervisor!")?
        .run()
        .await
        .context("Supervisor exited with an error!")
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
