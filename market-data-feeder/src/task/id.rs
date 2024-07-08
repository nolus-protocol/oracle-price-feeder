use std::{
    borrow::Cow, collections::btree_map::Entry as BTreeMapEntry, sync::Arc,
};

use anyhow::{bail, Context as _};

use chain_ops::{
    contract::admin::{Dex, Protocol, ProtocolContracts},
    env::ReadFromVar,
    node,
    supervisor::TaskCreationContext,
    task::application_defined,
    tx::ExecuteTemplate,
};

use crate::{
    oracle::Oracle,
    providers::{astroport::Astroport, osmosis::Osmosis, Provider},
    ApplicationDefinedContext,
};

use super::{Base, Task};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Id {
    protocol: Arc<str>,
}

impl Id {
    pub const fn new(protocol: Arc<str>) -> Self {
        Self { protocol }
    }

    fn dex_node_grpc_var(mut network: String) -> anyhow::Result<String> {
        const VAR_SUFFIX: &str = "__NODE_GRPC";

        if network.is_empty() {
            bail!("Protocol's network identifier is zero-length!");
        }

        let mut search_index = 1;

        while let Some(index) = network[search_index..]
            .find(|ch: char| ch.is_ascii_uppercase())
            .map(|index| search_index + index)
        {
            network.insert(index, '_');

            search_index = index + 2;
        }

        network = network.to_ascii_uppercase().replace('-', "_");

        network.reserve_exact(VAR_SUFFIX.len());

        network.push_str(VAR_SUFFIX);

        Ok(network)
    }

    const fn dex_name(dex: &Dex) -> &'static str {
        match dex {
            Dex::Astroport { .. } => "Astroport",
            Dex::Osmosis => "Osmosis",
        }
    }

    fn construct_provider(dex: Dex) -> Provider {
        match dex {
            Dex::Astroport { router_address } => {
                Provider::Astroport(Astroport::new(router_address))
            },
            Dex::Osmosis => Provider::Osmosis(Osmosis::new()),
        }
    }
}

impl application_defined::Id for Id {
    type TaskCreationContext = ApplicationDefinedContext;

    type Task = Task;

    #[inline]
    fn protocol(&self) -> Option<&Arc<str>> {
        Some(&self.protocol)
    }

    #[inline]
    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(self.protocol.to_string())
    }

    async fn into_task(
        self,
        mut task_creation_context: TaskCreationContext<'_, Task>,
    ) -> anyhow::Result<Task> {
        let Protocol {
            network,
            dex,
            contracts:
                ProtocolContracts {
                    oracle: oracle_address,
                },
        } = task_creation_context
            .admin_contract()
            .protocol(&self.protocol)
            .await
            .with_context(|| {
                format!(
                    "Failed to query protocol's information! Protocol={}",
                    self.protocol
                )
            })?;

        let node_client = task_creation_context.node_client().clone();

        let dex_node_client = {
            let entry = task_creation_context
                .application_defined()
                .dex_node_clients
                .entry(network.clone());

            match entry {
                BTreeMapEntry::Vacant(entry) => entry.insert(
                    node::Client::connect(
                        &Self::dex_node_grpc_var(network.clone())
                            .and_then(String::read_from_var)?,
                    )
                    .await?,
                ),
                BTreeMapEntry::Occupied(entry) => entry.into_mut(),
            }
            .clone()
        };

        task_creation_context
            .application_defined()
            .dex_node_clients
            .insert(network, dex_node_client.clone());

        Oracle::new(
            node_client.clone().query_wasm(),
            oracle_address.clone(),
            task_creation_context
                .application_defined()
                .update_currencies_interval,
        )
        .await
        .map(|oracle| Base {
            protocol: self.protocol.clone(),
            node_client,
            oracle,
            dex_node_client,
            source: format!(
                "{}; Protocol={}",
                Self::dex_name(&dex),
                self.protocol,
            )
            .into(),
            duration_before_start: task_creation_context
                .application_defined()
                .duration_before_start,
            execute_template: ExecuteTemplate::new(
                task_creation_context.signer_address().into(),
                oracle_address,
            ),
            idle_duration: task_creation_context.idle_duration(),
            timeout_duration: task_creation_context.timeout_duration(),
            hard_gas_limit: task_creation_context
                .application_defined()
                .gas_limit,
            transaction_tx: task_creation_context.transaction_tx().clone(),
        })
        .map(|base| Task {
            base,
            provider: Self::construct_provider(dex),
        })
    }
}
