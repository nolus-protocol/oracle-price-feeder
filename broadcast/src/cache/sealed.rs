use std::{cell::Cell, collections::BTreeMap, num::NonZeroU64};

use chain_comms::reexport::cosmrs::Any as ProtobufAny;

use crate::mode;

#[must_use]
pub struct TxRequest<Impl>
where
    Impl: mode::Impl,
{
    pub(crate) messages: Vec<ProtobufAny>,
    pub(crate) fallback_gas_limit: NonZeroU64,
    pub(crate) hard_gas_limit: NonZeroU64,
    pub(crate) expiration: Impl::Expiration,
}

pub type TxRequests<Impl> = BTreeMap<usize, Cell<Option<TxRequest<Impl>>>>;
