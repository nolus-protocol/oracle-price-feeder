use std::{
    cell::Cell,
    collections::btree_map::{BTreeMap, Entry as BTreeMapEntry},
};

use tokio::sync::mpsc::{error::TryRecvError, UnboundedReceiver};
use tracing::warn;

pub(crate) use sealed::{TxRequest, TxRequests};

use crate::generators::TxRequest as ReceivedTxRequest;
use crate::impl_variant::{self, FilterResult, PurgeResult};

mod sealed;

#[inline]
pub(crate) fn get_next<Impl>(
    requests_cache: &mut TxRequests<Impl>,
    next_sender_id: usize,
) -> Option<GetNextResult<Impl>>
where
    Impl: impl_variant::Impl,
{
    requests_cache
        .range(next_sender_id..)
        .chain(requests_cache.range(..next_sender_id))
        .find_map(|(&sender_id, slot)| {
            slot.replace(None).map(move |tx_request| GetNextResult {
                sender_id,
                tx_request,
            })
        })
}

#[inline]
pub(crate) async fn purge_and_update<Impl>(
    tx_receiver: &mut UnboundedReceiver<ReceivedTxRequest<Impl>>,
    requests_cache: &mut BTreeMap<usize, Cell<Option<TxRequest<Impl>>>>,
) -> Result<(), ChannelClosed>
where
    Impl: impl_variant::Impl,
{
    let mut recv_result: Result<ReceivedTxRequest<Impl>, TryRecvError> =
        if matches!(Impl::purge_cache(requests_cache), PurgeResult::Exhausted) {
            tx_receiver.recv().await.ok_or(TryRecvError::Disconnected)
        } else {
            tx_receiver.try_recv()
        };

    loop {
        match recv_result {
            Ok(ReceivedTxRequest {
                sender_id,
                messages,
                fallback_gas_limit,
                hard_gas_limit,
                expiration,
            }) => {
                if matches!(Impl::filter(&expiration), FilterResult::NotExpired) {
                    let tx_request = Some(TxRequest {
                        messages,
                        fallback_gas_limit,
                        hard_gas_limit,
                        expiration,
                    });

                    match requests_cache.entry(sender_id) {
                        BTreeMapEntry::Occupied(entry) => {
                            entry.into_mut().set(tx_request);
                        }
                        BTreeMapEntry::Vacant(entry) => {
                            entry.insert(Cell::new(tx_request));
                        }
                    }
                } else {
                    warn!("Transaction already expired. Skipping over.");
                }
            }
            Err(TryRecvError::Empty) => {
                return Ok(());
            }
            Err(TryRecvError::Disconnected) => {
                return Err(ChannelClosed);
            }
        }

        recv_result = tx_receiver.try_recv();
    }
}

pub(crate) struct ChannelClosed;

pub(crate) struct GetNextResult<Impl>
where
    Impl: impl_variant::Impl,
{
    pub(crate) sender_id: usize,
    pub(crate) tx_request: TxRequest<Impl>,
}
