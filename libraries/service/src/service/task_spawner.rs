use std::future::Future;

use thiserror::Error;
use tokio::task::AbortHandle;

use crate::service::TaskHandlesSender;

#[derive(Clone)]
pub struct TaskSpawner<Identifier, Output>
where
    Identifier: Send + 'static,
    Output: Send + 'static,
{
    task_handles_tx: TaskHandlesSender<Identifier, Output>,
}

impl<Identifier, Output> TaskSpawner<Identifier, Output>
where
    Identifier: Send + 'static,
    Output: Send + 'static,
{
    pub(super) const fn new(
        task_handles_tx: TaskHandlesSender<Identifier, Output>,
    ) -> Self {
        Self { task_handles_tx }
    }

    pub async fn spawn<F>(
        &self,
        identifier: Identifier,
        future: F,
    ) -> Result<CancellationToken, ServiceStopped>
    where
        F: Future<Output = Output> + Send + 'static,
    {
        let join_handle = tokio::spawn(future);

        let cancellation_token =
            CancellationToken::new(join_handle.abort_handle());

        channel::Sender::send(&self.task_handles_tx, (identifier, join_handle))
            .await
            .map(|()| cancellation_token)
            .map_err(|channel::Closed {}| ServiceStopped {})
    }
}

/// **Note:** Dropping the cancellation token will result in the cancellation of
/// the task.
pub struct CancellationToken {
    abort_handle: AbortHandle,
}

impl CancellationToken {
    const fn new(abort_handle: AbortHandle) -> Self {
        Self { abort_handle }
    }
}

impl Drop for CancellationToken {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

#[derive(Debug, Error)]
#[error("Service is stopped!")]
pub struct ServiceStopped;
