use std::future::Future;

use anyhow::Result;

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
pub fn spawn_with_id<Id, T>(
    task_set: &mut TaskSet<Id, Result<()>>,
    id: Id,
    task: T,
    runnable_state: RunnableState,
) where
    T: Task<Id>,
{
    task_set.add_handle(id, tokio::spawn(task.run(runnable_state)));
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
pub fn spawn_new<Id, T>(task_set: &mut TaskSet<Id, Result<()>>, task: T)
where
    T: Task<Id>,
{
    spawn(task_set, task, RunnableState::New);
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
pub fn spawn_restarting<Id, T>(task_set: &mut TaskSet<Id, Result<()>>, task: T)
where
    T: Task<Id>,
{
    spawn(task_set, task, RunnableState::Restart);
}
