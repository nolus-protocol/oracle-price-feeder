use std::{collections::BTreeMap, convert::Infallible, num::NonZeroU64};

use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::Instant,
};

use chain_comms::{
    interact::{commit, TxHash},
    reexport::cosmrs::Any as ProtobufAny,
};

use crate::mode::{self, Blocking, NonBlocking};

#[must_use]
#[inline]
pub fn new_results_channel() -> (CommitResultSender, CommitResultReceiver) {
    unbounded_channel()
}

pub enum CommitErrorType {
    InvalidAccountSequence,
    Unknown,
}

pub struct CommitError {
    pub r#type: CommitErrorType,
    pub tx_response: commit::Response,
}

pub type CommitResult = Result<TxHash, CommitError>;

pub type CommitResultSender = UnboundedSender<CommitResult>;

pub type CommitResultReceiver = UnboundedReceiver<CommitResult>;

#[must_use]
pub struct SpawnResult {
    pub(crate) tx_generators_set: JoinSet<Infallible>,
    pub(crate) tx_result_senders: BTreeMap<usize, CommitResultSender>,
}

impl SpawnResult {
    pub const fn new(
        tx_generators_set: JoinSet<Infallible>,
        tx_result_senders: BTreeMap<usize, CommitResultSender>,
    ) -> Self {
        Self {
            tx_generators_set,
            tx_result_senders,
        }
    }
}

#[must_use]
pub struct TxRequest<Impl: mode::Impl> {
    pub(crate) sender_id: usize,
    pub(crate) messages: Vec<ProtobufAny>,
    pub(crate) fallback_gas_limit: NonZeroU64,
    pub(crate) hard_gas_limit: NonZeroU64,
    pub(crate) expiration: Impl::Expiration,
}

impl TxRequest<Blocking> {
    pub const fn new(
        sender_id: usize,
        messages: Vec<ProtobufAny>,
        fallback_gas_limit: NonZeroU64,
        hard_gas_limit: NonZeroU64,
    ) -> Self {
        Self {
            sender_id,
            messages,
            fallback_gas_limit,
            hard_gas_limit,
            expiration: (),
        }
    }
}

impl TxRequest<NonBlocking> {
    pub const fn new(
        sender_id: usize,
        messages: Vec<ProtobufAny>,
        fallback_gas_limit: NonZeroU64,
        hard_gas_limit: NonZeroU64,
        expiration: Instant,
    ) -> Self {
        Self {
            sender_id,
            messages,
            fallback_gas_limit,
            hard_gas_limit,
            expiration,
        }
    }
}

pub type TxRequestSender<Impl> = UnboundedSender<TxRequest<Impl>>;
