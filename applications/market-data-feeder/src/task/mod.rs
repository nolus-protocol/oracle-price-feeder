use std::{sync::Arc, time::Duration};

use anyhow::Result;
use cosmrs::Gas;

use chain_ops::{node, tx::ExecuteTemplate};
use channel::unbounded;
use dex::providers::astroport::Astroport;
use dex::providers::osmosis::Osmosis;
use service::task::{
    application_defined, Runnable, RunnableState, TimeBasedExpiration,
    TxPackage,
};

use crate::oracle::Oracle;

pub use self::{
    context::ApplicationDefined as ApplicationDefinedContext, id::Id,
};

mod context;
mod id;
mod provider;

pub enum Task {
    Astroport(TaskWithProvider<Astroport>),
    Osmosis(TaskWithProvider<Osmosis>),
}

impl Runnable for Task {
    async fn run(self, state: RunnableState) -> Result<()> {
        match self {
            Self::Astroport(task) => task.run(state).await,
            Self::Osmosis(task) => task.run(state).await,
        }
    }
}

impl application_defined::Task for Task {
    type TxExpiration = TimeBasedExpiration;

    type Id = Id;

    #[inline]
    fn id(&self) -> Self::Id {
        Id::new(
            match self {
                Self::Astroport(task) => &task.protocol,
                Self::Osmosis(task) => &task.protocol,
            }
            .clone(),
        )
    }

    #[inline]
    fn protocol_task_set_ids(
        protocol: Arc<str>,
    ) -> impl Iterator<Item = Self::Id> + Send + 'static {
        [Id::new(protocol)].into_iter()
    }
}

pub struct TaskWithProvider<Dex>
where
    Dex: dex::provider::Dex,
{
    pub protocol: Arc<str>,
    pub source: Arc<str>,
    pub query_tx: node::QueryTx,
    pub dex_node_client: node::Client,
    pub duration_before_start: Duration,
    pub execute_template: ExecuteTemplate,
    pub idle_duration: Duration,
    pub timeout_duration: Duration,
    pub hard_gas_limit: Gas,
    pub oracle: Oracle<Dex>,
    pub provider: Dex,
    pub transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
}
