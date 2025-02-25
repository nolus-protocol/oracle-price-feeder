use std::{collections::BTreeSet, future::Future, sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use tokio::time::sleep;

use channel::{bounded, Sender};
use contract::{Admin, CheckedContract};
use task::RunnableState;
use task_set::TaskSet;
use tx::{TxExpiration, TxPackage};

macro_rules! log {
    ($macro:ident![$protocol:expr]($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "protocol-watcher",
            protocol = %$protocol,
            $($body)+
        );
    };
}

#[derive(Debug)]
pub enum Command {
    ProtocolAdded(Arc<str>),
    ProtocolRemoved(Arc<str>),
}

#[derive(Clone)]
#[must_use]
pub struct State {
    pub admin_contract: CheckedContract<Admin>,
    pub action_tx: bounded::Sender<Command>,
}

impl State {
    pub fn run(
        self,
        runnable_state: RunnableState,
    ) -> impl Future<Output = Result<()>> + Sized + use<> {
        let Self {
            admin_contract,
            action_tx,
        } = self;

        ProtocolWatcher::new(admin_contract, BTreeSet::new(), action_tx)
            .run(runnable_state)
    }
}

#[inline]
pub fn action_handler<
    Id,
    State,
    TxSender,
    TxExpiration: self::TxExpiration,
    AddProtocol,
    RemoveProtocol,
>(
    tx: TxSender,
    mut add_protocol: AddProtocol,
    mut remove_protocol: RemoveProtocol,
) -> impl for<'r> AsyncFnMut(
    &'r mut TaskSet<Id, Result<()>>,
    State,
    Command,
) -> Result<State>
where
    TxSender: Sender<Value = TxPackage<TxExpiration>>,
    AddProtocol: for<'r, 't> AsyncFnMut(
        &'r mut TaskSet<Id, Result<()>>,
        State,
        Arc<str>,
        &'t TxSender,
    ) -> Result<State>,
    RemoveProtocol: for<'r> AsyncFnMut(
        &'r mut TaskSet<Id, Result<()>>,
        State,
        Arc<str>,
    ) -> Result<State>,
{
    async move |task_set, state, command| match command {
        Command::ProtocolAdded(protocol) => {
            add_protocol(task_set, state, protocol, &tx).await
        },
        Command::ProtocolRemoved(protocol) => {
            remove_protocol(task_set, state, protocol).await
        },
    }
}

#[must_use]
struct ProtocolWatcher {
    admin_contract: CheckedContract<Admin>,
    protocol_tasks: BTreeSet<Arc<str>>,
    command_tx: bounded::Sender<Command>,
}

impl ProtocolWatcher {
    const fn new(
        admin_contract: CheckedContract<Admin>,
        protocol_tasks: BTreeSet<Arc<str>>,
        command_tx: bounded::Sender<Command>,
    ) -> Self {
        Self {
            admin_contract,
            protocol_tasks,
            command_tx,
        }
    }

    async fn run(mut self, _: RunnableState) -> Result<()> {
        const IDLE_DURATION: Duration = Duration::from_secs(15);

        loop {
            let active_protocols = self
                .admin_contract
                .protocols()
                .await
                .context("Failed to fetch protocols!")?
                .into_vec()
                .into_iter()
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

            sleep(IDLE_DURATION).await;
        }
    }
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
