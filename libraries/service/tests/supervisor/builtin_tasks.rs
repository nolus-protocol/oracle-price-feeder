use std::{
    collections::BTreeMap,
    future::pending,
    sync::{atomic::Ordering, Arc},
};

use anyhow::Result;
use tokio::task::yield_now;
use tracing::info;

use service::task::{
    protocol_watcher, BalanceReporter, Broadcast, BuiltIn, Id, NoExpiration,
    ProtocolWatcher, Runnable, State, TxPackage,
};
use task::RunnableState;

use super::Context;

pub(crate) struct TestingBalanceReporter;

impl Drop for TestingBalanceReporter {
    #[inline]
    fn drop(&mut self) {
        info!("Balance reported stopped.");
    }
}

impl Runnable for TestingBalanceReporter {
    #[inline]
    async fn run(self, _: RunnableState) -> Result<()> {
        info!("Balance reporter started.");

        pending().await
    }
}

impl BuiltIn for TestingBalanceReporter {
    type ServiceConfiguration = Context;
}

impl BalanceReporter for TestingBalanceReporter {
    #[inline]
    fn new(_: &Self::ServiceConfiguration) -> Self {
        const { Self {} }
    }
}

pub(crate) struct TestingBroadcast;

impl Drop for TestingBroadcast {
    #[inline]
    fn drop(&mut self) {
        info!("Broadcast stopped.");
    }
}
impl Runnable for TestingBroadcast {
    #[inline]
    async fn run(self, _: RunnableState) -> Result<()> {
        info!("Broadcast started.");

        pending().await
    }
}
impl BuiltIn for TestingBroadcast {
    type ServiceConfiguration = Context;
}

impl Broadcast for TestingBroadcast {
    type TxExpiration = NoExpiration;

    #[inline]
    fn new(
        _: &Self::ServiceConfiguration,
        _: channel::unbounded::Receiver<TxPackage<Self::TxExpiration>>,
    ) -> Self {
        const { Self {} }
    }
}

pub(crate) struct TestingProtocolWatcher {
    service_configuration: Context,
    command_tx: channel::bounded::Sender<protocol_watcher::Command>,
}

impl Drop for TestingProtocolWatcher {
    #[inline]
    fn drop(&mut self) {
        info!("Protocol watcher stopped.");
    }
}

impl Runnable for TestingProtocolWatcher {
    #[inline]
    async fn run(self, _: RunnableState) -> Result<()> {
        info!("Protocol watcher started.");

        let initial_count = self
            .service_configuration
            .application_defined_tasks_count
            .load(Ordering::Acquire);

        if initial_count == 0 {
            let protocols: [Arc<str>; 2] =
                std::array::from_fn(|i| (i + 1).to_string().into());

            for protocol in protocols.iter().cloned() {
                info!(%protocol, "Starting application defined task.");

                self.command_tx
                    .send(protocol_watcher::Command::ProtocolAdded(protocol))
                    .await?;
            }

            loop {
                let current_count = self
                    .service_configuration
                    .application_defined_tasks_count
                    .load(Ordering::Acquire);

                if protocols.len() == current_count {
                    break;
                }

                yield_now().await;
            }

            info!("Protocols spawned.");

            for (count, protocol) in protocols.into_iter().enumerate().rev() {
                info!(%protocol, "Stopping application defined task.");

                self.command_tx
                    .send(protocol_watcher::Command::ProtocolRemoved(protocol))
                    .await?;

                loop {
                    let current_count = self
                        .service_configuration
                        .application_defined_tasks_count
                        .load(Ordering::Acquire);

                    if current_count == count {
                        break;
                    }

                    yield_now().await;
                }
            }

            info!("Protocols stopped.");

            () = self.service_configuration.notify.notify_waiters();
        }

        pending().await
    }
}

impl BuiltIn for TestingProtocolWatcher {
    type ServiceConfiguration = Context;
}

impl ProtocolWatcher for TestingProtocolWatcher {
    #[inline]
    fn new<ApplicationDefined>(
        service_configuration: &Self::ServiceConfiguration,
        _: &BTreeMap<Id<ApplicationDefined>, State>,
        command_tx: channel::bounded::Sender<protocol_watcher::Command>,
    ) -> Self
    where
        ApplicationDefined: service::task::application_defined::Id,
    {
        let service_configuration = service_configuration.clone();

        Self {
            service_configuration,
            command_tx,
        }
    }
}
