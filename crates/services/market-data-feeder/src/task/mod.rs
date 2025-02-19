use std::{sync::Arc, time::Duration};

use anyhow::Result;
use cosmrs::Gas;

use chain_ops::{node, tx::ExecuteTemplate};
use channel::unbounded;
use dex::{
    provider::Dex,
    providers::{astroport::Astroport, osmosis::Osmosis},
    CurrencyPairs,
};
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
    Astroport(TaskWithProvider<AstroportOracle>),
    Osmosis(TaskWithProvider<OsmosisOracle>),
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

pub struct TaskWithProvider<Instance>
where
    Instance: Contract,
{
    protocol: Arc<str>,
    source: Arc<str>,
    node_client: node::Client,
    dex_node_client: node::Client,
    duration_before_start: Duration,
    execute_template: ExecuteTemplate,
    idle_duration: Duration,
    timeout_duration: Duration,
    hard_gas_limit: Gas,
    oracle: Oracle<Instance::Dex>,
    provider: Instance::Dex,
    transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
}

pub trait Contract {
    type Dex: Dex;

    fn fetch_currencies(&mut self) -> Result<()>;

    fn fetch_currency_pairs(&mut self) -> Result<CurrencyPairs<Self::Dex>>;
}

pub enum AstroportOracle {}

impl Contract for AstroportOracle {
    type Dex = Astroport;

    fn fetch_currencies(&mut self) -> Result<()> {
        todo!()
    }

    fn fetch_currency_pairs(&mut self) -> Result<CurrencyPairs<Self::Dex>> {
        todo!()
    }
}

pub enum OsmosisOracle {}

impl Contract for OsmosisOracle {
    type Dex = Osmosis;

    fn fetch_currencies(&mut self) -> Result<()> {
        todo!()
    }

    fn fetch_currency_pairs(&mut self) -> Result<CurrencyPairs<Self::Dex>> {
        todo!()
    }
}
