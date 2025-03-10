use std::sync::Arc;

use task::Task;
use tx::TimeBasedExpiration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Id {
    BalanceReporter,
    Broadcaster,
    ProtocolWatcher,
    PriceFetcher { protocol: Arc<str> },
}

impl Task<Id> for balance_reporter::State {
    const ID: Id = Id::BalanceReporter;
}

impl Task<Id> for broadcaster::State<TimeBasedExpiration> {
    const ID: Id = Id::Broadcaster;
}

impl Task<Id> for protocol_watcher::State {
    const ID: Id = Id::ProtocolWatcher;
}
