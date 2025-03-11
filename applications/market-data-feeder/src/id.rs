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
    #[inline]
    fn id(&self) -> Id {
        Id::BalanceReporter
    }
}

impl Task<Id> for broadcaster::State<TimeBasedExpiration> {
    #[inline]
    fn id(&self) -> Id {
        Id::Broadcaster
    }
}

impl Task<Id> for protocol_watcher::State {
    #[inline]
    fn id(&self) -> Id {
        Id::ProtocolWatcher
    }
}
