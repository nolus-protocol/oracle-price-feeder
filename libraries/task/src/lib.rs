use std::time::Duration;

use anyhow::Result;
use tokio::time::sleep;

use task_set::TaskSet;

pub enum RunnableState {
    New,
    Restart,
}

pub trait Run {
    fn run(
        self,
        state: RunnableState,
    ) -> impl Future<Output = Result<()>> + Send + 'static;
}

pub trait Task<Id>: Run {
    fn id(&self) -> Id;
}

#[inline]
pub fn spawn_new<Id, T>(task_set: &mut TaskSet<Id, Result<()>>, task: T)
where
    T: Task<Id>,
{
    spawn_future(task_set, task.id(), task.run(RunnableState::New));
}

#[inline]
pub fn spawn_restarting<Id, T>(task_set: &mut TaskSet<Id, Result<()>>, task: T)
where
    T: Task<Id>,
{
    spawn_future(task_set, task.id(), task.run(RunnableState::Restart));
}

#[inline]
pub fn spawn_restarting_delayed<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    task: T,
    delay: Duration,
) where
    T: Task<Id>,
{
    spawn_future(task_set, task.id(), {
        let future = task.run(RunnableState::Restart);

        async move {
            sleep(delay).await;

            future.await
        }
    });
}

fn spawn_future<Id, F>(task_set: &mut TaskSet<Id, F::Output>, id: Id, future: F)
where
    F: Future<Output: Send + 'static> + Send + 'static,
{
    task_set.add_handle(id, tokio::spawn(future));
}
