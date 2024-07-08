use std::{collections::BTreeSet, sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use tokio::{sync::mpsc, time::sleep};

use crate::contract::Admin as AdminContract;

use super::Runnable;

macro_rules! log {
    ($macro:ident![$protocol:expr]($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "protocol-watcher",
            protocol = %$protocol,
            $($body)+
        );
    };
}

#[must_use]
pub struct ProtocolWatcher {
    admin_contract: AdminContract,
    protocol_tasks: BTreeSet<Arc<str>>,
    command_tx: mpsc::Sender<Command>,
}

impl ProtocolWatcher {
    pub const fn new(
        admin_contract: AdminContract,
        protocol_tasks: BTreeSet<Arc<str>>,
        command_tx: mpsc::Sender<Command>,
    ) -> Self {
        Self {
            admin_contract,
            protocol_tasks,
            command_tx,
        }
    }
}

impl Runnable for ProtocolWatcher {
    async fn run(mut self) -> Result<()> {
        loop {
            let active_protocols = self
                .admin_contract
                .protocols()
                .await
                .context("Failed to fetch protocols!")?
                .into_iter()
                .map(Into::into)
                .collect();

            for command in
                protocols_diff_commands(&self.protocol_tasks, &active_protocols)
            {
                match &command {
                    Command::ProtocolAdded(protocol) => {
                        log!(info![protocol]("Protocol added."));

                        assert!(self.protocol_tasks.insert(protocol.clone()));
                    },
                    Command::ProtocolRemoved(protocol) => {
                        log!(info![protocol]("Protocol removed."));

                        _ = self.protocol_tasks.remove(protocol);
                    },
                }

                self.command_tx.send(command).await?;
            }

            sleep(Duration::from_secs(15)).await;
        }
    }
}

#[derive(Debug)]
pub enum Command {
    ProtocolAdded(Arc<str>),
    ProtocolRemoved(Arc<str>),
}

fn protocols_diff_commands(
    protocols: &BTreeSet<Arc<str>>,
    active_protocols: &BTreeSet<Arc<str>>,
) -> Vec<Command> {
    active_protocols
        .difference(protocols)
        .cloned()
        .map(Command::ProtocolAdded)
        .chain(
            protocols
                .difference(active_protocols)
                .cloned()
                .map(Command::ProtocolRemoved),
        )
        .collect()
}
