use std::{sync::Arc, time::Duration};

use anyhow::Result;
use cosmrs::Gas;

use chain_ops::{
    channel::unbounded,
    node,
    task::{
        application_defined, Runnable, RunnableState, TimeBasedExpiration,
        TxPackage,
    },
    tx::ExecuteTemplate,
};
use dex::{oracle::Oracle, provider::Dex, providers::Provider as DexProvider};

use self::provider::Provider;
pub use self::{
    context::ApplicationDefined as ApplicationDefinedContext, id::Id,
};

mod context;
mod id;
mod provider;

pub struct Task {
    base: Base,
    provider: DexProvider,
}

impl Runnable for Task {
    async fn run(self, state: RunnableState) -> Result<()> {
        match self.provider {
            DexProvider::Astroport(provider) => {
                Provider::new(self.base, provider).run(state).await
            },
            DexProvider::Osmosis(provider) => {
                Provider::new(self.base, provider).run(state).await
            },
        }
    }
}

impl application_defined::Task for Task {
    type TxExpiration = TimeBasedExpiration;

    type Id = Id;

    #[inline]
    fn id(&self) -> Self::Id {
        Id::new(self.base.protocol.clone())
    }

    #[inline]
    fn protocol_task_set_ids(
        protocol: Arc<str>,
    ) -> impl Iterator<Item = Self::Id> + Send + 'static {
        [Id::new(protocol)].into_iter()
    }
}

struct Base {
    protocol: Arc<str>,
    node_client: node::Client,
    oracle: Oracle,
    dex_node_client: node::Client,
    source: Arc<str>,
    duration_before_start: Duration,
    execute_template: ExecuteTemplate,
    idle_duration: Duration,
    timeout_duration: Duration,
    hard_gas_limit: Gas,
    transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
}
