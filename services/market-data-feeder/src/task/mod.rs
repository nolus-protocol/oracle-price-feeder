use std::{sync::Arc, time::Duration};

use anyhow::Result;
use cosmrs::Gas;

use chain_ops::{
    channel::unbounded,
    node,
    task::{application_defined, Runnable, TxPackage, WithExpiration},
    tx::ExecuteTemplate,
};

use market_data_feeder::{oracle::Oracle, providers};

pub(crate) use self::id::Id;
use self::provider::Provider;

mod id;
mod provider;

pub(crate) struct Task {
    base: Base,
    provider: providers::Provider,
}

impl Runnable for Task {
    async fn run(self) -> Result<()> {
        match self.provider {
            providers::Provider::Astroport(provider) => {
                Provider::new(self.base, provider).run().await
            },
            providers::Provider::Osmosis(provider) => {
                Provider::new(self.base, provider).run().await
            },
        }
    }
}

impl application_defined::Task for Task {
    type TxExpiration = WithExpiration;

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
    transaction_tx: unbounded::Sender<TxPackage<WithExpiration>>,
}
