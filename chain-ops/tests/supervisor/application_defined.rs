use std::{
    borrow::Cow,
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::Result;
use tracing::info;

use chain_ops::{
    channel,
    task::{application_defined, NoExpiration, Runnable, TxPackage},
};

use super::Context;

pub(crate) struct Task {
    protocol: Arc<str>,
    app_defined_tasks_count: Arc<AtomicUsize>,
}

impl Drop for Task {
    fn drop(&mut self) {
        info!(protocol = %self.protocol, "Task stopped.");

        self.app_defined_tasks_count.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Runnable for Task {
    async fn run(self) -> Result<()> {
        info!(protocol = %self.protocol, "Task started.");

        self.app_defined_tasks_count.fetch_add(1, Ordering::AcqRel);

        pending().await
    }
}

impl application_defined::Task for Task {
    type TxExpiration = NoExpiration;

    type Id = Id;

    fn id(&self) -> Self::Id {
        Self::Id {
            protocol: self.protocol.clone(),
        }
    }

    #[inline]
    fn protocol_task_set_ids(
        protocol: Arc<str>,
    ) -> impl Iterator<Item = Self::Id> + Send + 'static {
        [Self::Id { protocol }].into_iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Id {
    protocol: Arc<str>,
}

impl application_defined::Id for Id {
    type ServiceConfiguration = Context;

    type TaskCreationContext = ();

    type Task = Task;

    fn protocol(&self) -> Option<&Arc<str>> {
        Some(&self.protocol)
    }

    fn name(&self) -> Cow<'static, str> {
        self.protocol.to_string().into()
    }

    async fn into_task<'r>(
        self,
        service_configuration: &'r mut Self::ServiceConfiguration,
        &mut (): &'r mut Self::TaskCreationContext,
        _: &'r channel::unbounded::Sender<TxPackage<NoExpiration>>,
    ) -> Result<Self::Task> {
        Ok(Self::Task {
            protocol: self.protocol.clone(),
            app_defined_tasks_count: service_configuration
                .application_defined_tasks_count
                .clone(),
        })
    }
}
