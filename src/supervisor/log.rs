use std::fmt::Display;

use anyhow::Result;
use tokio::task::JoinError;

use crate::task::application_defined::Id;

#[inline]
pub fn balance_reporter_result(result: Result<Result<()>, JoinError>) {
    () = log_task_result("Balance Reporter", result);
}

#[inline]
pub fn broadcast_result(result: Result<Result<()>, JoinError>) {
    () = log_task_result("Broadcast", result);
}

#[inline]
pub fn protocol_watcher_result(result: Result<Result<()>, JoinError>) {
    () = log_task_result("Protocol Watcher", result);
}

#[inline]
pub fn application_defined_result<T>(
    id: &T,
    result: Result<Result<()>, JoinError>,
) where
    T: Id,
{
    () = log_task_result(id.name(), result);
}

fn log_task_result<TaskId>(
    task_id: TaskId,
    result: Result<Result<()>, JoinError>,
) where
    TaskId: Display,
{
    match result.map_err(JoinError::try_into_panic) {
        Ok(Ok(())) => {
            log!(info!(
                task = %task_id,
                "Exited without an error."
            ));
        },
        Ok(Err(error)) => {
            log!(error!(
                task = %task_id,
                ?error,
                "Exited with an error!"
            ));
        },
        Err(Ok(_)) => {
            log!(error!(
                task = %task_id,
                "Task panicked!"
            ));
        },
        Err(Err(error)) if error.is_cancelled() => {
            log!(error!(
                task = %task_id,
                "Task cancelled!"
            ));
        },
        Err(Err(error)) => {
            log!(error!(
                task = %task_id,
                ?error,
                "Exited in an unknown way!"
            ));
        },
    }
}
