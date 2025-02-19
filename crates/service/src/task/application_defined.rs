use std::{borrow::Cow, fmt::Debug, future::Future, sync::Arc};

use anyhow::Result;

use super::{Runnable, TxExpiration, TxPackage};

pub trait Task: Runnable + Send + Sized + 'static {
    type TxExpiration: TxExpiration;

    type Id: Id<Task = Self>;

    fn id(&self) -> Self::Id;

    fn protocol_task_set_ids(
        protocol: Arc<str>,
    ) -> impl Iterator<Item = Self::Id> + Send + 'static;
}

pub trait Id: Debug + Clone + Ord + Send + Sized + 'static {
    type ServiceConfiguration: Send + 'static;

    type TaskCreationContext: Send + 'static;

    type Task: Task<Id = Self>;

    fn protocol(&self) -> Option<&Arc<str>>;

    fn name(&self) -> Cow<'static, str>;

    fn into_task<'r>(
        self,
        service_configuration: &'r mut Self::ServiceConfiguration,
        task_creation_context: &'r mut Self::TaskCreationContext,
        transaction_tx: &'r channel::unbounded::Sender<
            TxPackage<<Self::Task as Task>::TxExpiration>,
        >,
    ) -> impl Future<Output = Result<Self::Task>> + Send + 'r;
}
