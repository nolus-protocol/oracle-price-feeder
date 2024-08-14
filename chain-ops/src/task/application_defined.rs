use std::{borrow::Cow, fmt::Debug, future::Future, sync::Arc};

use anyhow::Result;

use crate::supervisor;

use super::{Runnable, TxExpiration};

pub trait Task: Runnable + Send + Sized + 'static {
    type TxExpiration: TxExpiration;

    type Id: Id<Task = Self>;

    fn id(&self) -> Self::Id;

    fn protocol_task_set_ids(
        protocol: Arc<str>,
    ) -> impl Iterator<Item = Self::Id> + Send + 'static;
}

pub trait Id: Debug + Clone + Ord + Send + Sized + 'static {
    type TaskCreationContext: Send + 'static;

    type Task: Task<Id = Self>;

    fn protocol(&self) -> Option<&Arc<str>>;

    fn name(&self) -> Cow<'static, str>;

    fn into_task(
        self,
        task_creation_context: supervisor::TaskCreationContext<'_, Self::Task>,
    ) -> impl Future<Output = Result<Self::Task>> + Send + '_;
}

pub type TaskCreationContext<T> = <<T as Task>::Id as Id>::TaskCreationContext;
