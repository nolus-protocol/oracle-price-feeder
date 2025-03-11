use std::{collections::BTreeSet, sync::Arc, time::Duration};

use anyhow::{Context as _, Result, bail};
use tokio::time::sleep;

use channel::{Sender, bounded};
use contract::{Admin, CheckedContract};
use task::{Run, RunnableState};
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
    admin_contract: CheckedContract<Admin>,
    action_tx: bounded::Sender<Command>,
}

impl State {
    #[inline]
    pub const fn new(
        admin_contract: CheckedContract<Admin>,
        action_tx: bounded::Sender<Command>,
    ) -> Self {
        Self {
            admin_contract,
            action_tx,
        }
    }
}

impl Run for State {
    async fn run(self, _: RunnableState) -> Result<()> {
        const IDLE_DURATION: Duration = Duration::from_secs(15);

        let Self {
            mut admin_contract,
            action_tx,
        } = self;

        let mut protocol_tasks = BTreeSet::new();

        loop {
            let active_protocols = admin_contract
                .protocols()
                .await
                .context("Failed to fetch protocols!")?
                .into_vec()
                .into_iter()
                .collect();

            for command in
                protocols_diff_commands(&protocol_tasks, &active_protocols)
            {
                match &command {
                    Command::ProtocolAdded(protocol) => {
                        log!(info![protocol]("Protocol added."));

                        if !protocol_tasks.insert(protocol.clone()) {
                            bail!("Protocol already exists!");
                        }
                    },
                    Command::ProtocolRemoved(protocol) => {
                        log!(info![protocol]("Protocol removed."));

                        _ = protocol_tasks.remove(protocol);
                    },
                }

                action_tx
                    .send(command)
                    .await
                    .context("Failed to send protocol change command!")?;
            }

            sleep(IDLE_DURATION).await;
        }
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
            add_protocol(task_set, state, protocol, &tx)
                .await
                .context("Failed to add protocol task!")
        },
        Command::ProtocolRemoved(protocol) => {
            remove_protocol(task_set, state, protocol)
                .await
                .context("Failed to remove protocol task!")
        },
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
