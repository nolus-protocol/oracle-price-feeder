use std::future::Future;

use tokio::{
    io, select,
    task::{JoinError, JoinHandle},
};

use crate::{
    channel::{self, bounded, Channel as _},
    task_set::TaskSet,
};

use self::task_spawner::TaskSpawner;

pub mod task_spawner;

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "service",
            $($body)+
        )
    };
}

#[derive(Debug)]
pub struct TaskResult<Id, Output> {
    pub identifier: Id,
    pub result: Result<Output, JoinError>,
}

type TaskHandlesChannel<Identifier, Output> =
    bounded::Channel<(Identifier, JoinHandle<Output>)>;

type TaskHandlesSender<Identifier, Output> =
    <TaskHandlesChannel<Identifier, Output> as channel::Channel>::Sender;

type TaskHandlesReceiver<Identifier, Output> =
    <TaskHandlesChannel<Identifier, Output> as channel::Channel>::Receiver;

type TaskResultsChannel<Id, Output> = bounded::Channel<TaskResult<Id, Output>>;

type TaskResultsSender<Id, Output> =
    <TaskResultsChannel<Id, Output> as channel::Channel>::Sender;

pub type TaskResultsReceiver<Id, Output> =
    <TaskResultsChannel<Id, Output> as channel::Channel>::Receiver;

pub enum ShutdownResult<T> {
    Exited(Result<T, JoinError>),
    StopSignalReceived,
}

pub async fn run<
    SpawnSupervisor,
    SupervisorFuture,
    TaskIdentifier,
    TaskOutput,
>(
    spawn_supervisor: SpawnSupervisor,
) -> io::Result<ShutdownResult<SupervisorFuture::Output>>
where
    SpawnSupervisor: FnOnce(
        TaskSpawner<TaskIdentifier, TaskOutput>,
        TaskResultsReceiver<TaskIdentifier, TaskOutput>,
    ) -> SupervisorFuture,
    SupervisorFuture: Future + Send + 'static,
    SupervisorFuture::Output: Send + 'static,
    TaskIdentifier: Unpin + Send + 'static,
    TaskOutput: Send + 'static,
{
    let (task_handles_tx, task_handles_rx) = TaskHandlesChannel::new();

    let (task_results_tx, task_results_rx) = TaskResultsChannel::new();

    let mut tasks_set = TaskSet::new();

    let event_loop = event_loop(
        tokio::spawn(spawn_supervisor(
            TaskSpawner::new(task_handles_tx),
            task_results_rx,
        )),
        &mut tasks_set,
        task_handles_rx,
        task_results_tx,
    );

    let supervisor_task_result = select! {
        biased;
        result = signal_handler() => {
            log!(info!("Stop signal received."));

            result.map(|()| ShutdownResult::StopSignalReceived)
        },
        result = event_loop => Ok(ShutdownResult::Exited(result)),
    };

    tasks_set.abort_all();

    while !tasks_set.is_empty() {
        let _: Option<(TaskIdentifier, Result<TaskOutput, JoinError>)> =
            tasks_set.join_next().await;
    }

    supervisor_task_result
}

#[inline]
fn signal_handler() -> impl Future<Output = io::Result<()>> {
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
    }

    #[cfg(unix)]
    {
        use std::io::Error as IoError;

        use anyhow::anyhow;
        use tokio::signal::unix::{signal, SignalKind};

        async {
            let mut interrupt = signal(SignalKind::interrupt())?;
            let mut quit = signal(SignalKind::quit())?;
            let mut terminate = signal(SignalKind::terminate())?;

            select! {
                biased;
                Some(()) = interrupt.recv() => Ok(()),
                Some(()) = quit.recv() => Ok(()),
                Some(()) = terminate.recv() => Ok(()),
                else => Err(IoError::other(anyhow!(
                    "All signal handlers closed and can't receive anymore!"
                ))),
            }
        }
    }
}

async fn event_loop<SupervisorOutput, TaskIdentifier, TaskOutput>(
    mut supervisor_handle: JoinHandle<SupervisorOutput>,
    tasks_set: &mut TaskSet<TaskIdentifier, TaskOutput>,
    mut task_handle_rx: TaskHandlesReceiver<TaskIdentifier, TaskOutput>,
    task_results_tx: TaskResultsSender<TaskIdentifier, TaskOutput>,
) -> Result<SupervisorOutput, JoinError>
where
    TaskIdentifier: Unpin + Send + 'static,
    TaskOutput: Send + 'static,
{
    loop {
        select! {
            biased;
            supervisor_task_result = &mut supervisor_handle => {
                break supervisor_task_result;
            }
            Some((identifier, task_handle)) = task_handle_rx.recv(),
                if !task_handle_rx.is_closed() => tasks_set.add_handle(
                identifier,
                task_handle,
            ),
            Some((identifier, result)) = tasks_set.join_next(),
                if !tasks_set.is_empty() => {
                let result = channel::Sender::send(
                    &task_results_tx,
                    TaskResult { identifier, result },
                )
                .await;

                match result {
                    Ok(()) => {}
                    Err(channel::Closed {}) => {
                        log!(error!(
                            "Channel for sending task results to supervisor \
                            closed!"
                        ));

                        drop(task_handle_rx);

                        drop(task_results_tx);

                        break supervisor_handle.await;
                    }
                }
            }
        }
    }
}
