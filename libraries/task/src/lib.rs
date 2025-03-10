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
    const ID: Id;
}

#[inline]
pub fn spawn<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    task: T,
    runnable_state: RunnableState,
) where
    T: Task<Id>,
{
    spawn_with_id(task_set, T::ID, task, runnable_state);
}

#[inline]
pub fn spawn_with_id<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    id: Id,
    task: T,
    runnable_state: RunnableState,
) where
    T: Task<Id>,
{
    spawn_future(task_set, id, task.run(runnable_state));
}

#[inline]
pub fn spawn_new<Id, T>(task_set: &mut TaskSet<Id, Result<()>>, task: T)
where
    T: Task<Id>,
{
    spawn_new_with_id(task_set, T::ID, task);
}

#[inline]
pub fn spawn_new_with_id<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    id: Id,
    task: T,
) where
    T: Task<Id>,
{
    spawn_with_id(task_set, id, task, RunnableState::New);
}

#[inline]
pub fn spawn_restarting<Id, T>(task_set: &mut TaskSet<Id, Result<()>>, task: T)
where
    T: Task<Id>,
{
    spawn_restarting_with_id(task_set, T::ID, task);
}

#[inline]
pub fn spawn_restarting_with_id<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    id: Id,
    task: T,
) where
    T: Task<Id>,
{
    spawn_with_id(task_set, id, task, RunnableState::Restart);
}

#[inline]
pub fn spawn_restarting_delayed<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    task: T,
    delay: Duration,
) where
    T: Task<Id>,
{
    spawn_restarting_with_id_delayed(task_set, T::ID, task, delay);
}

#[inline]
pub fn spawn_restarting_with_id_delayed<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    id: Id,
    task: T,
    delay: Duration,
) where
    T: Task<Id>,
{
    let future = task.run(RunnableState::Restart);

    spawn_future(task_set, id, async move {
        sleep(delay).await;

        future.await
    });
}

fn spawn_future<Id, F>(task_set: &mut TaskSet<Id, F::Output>, id: Id, future: F)
where
    F: Future<Output: Send + 'static> + Send + 'static,
{
    task_set.add_handle(id, tokio::spawn(future));
}
