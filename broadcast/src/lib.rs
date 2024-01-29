use std::{collections::btree_map::BTreeMap, future::poll_fn, task::Poll, time::Duration};

use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::{JoinError, JoinSet},
    time::{sleep, timeout, Instant},
};
use tracing::{error, info};

use chain_comms::{
    client::Client as NodeClient,
    config::Node as NodeConfig,
    interact::get_tx_response,
    reexport::cosmrs::tendermint::{abci::response::DeliverTx, Hash as TxHash},
    signer::Signer,
};

pub use self::impl_variant::{TimeInsensitive, TimeSensitive};
use self::{
    broadcast::BroadcastAndSendBackTxHash,
    config::Config,
    generators::{CommitResultSender, SpawnResult, TxRequest, TxRequestSender},
    impl_variant::FilterResult,
};

mod broadcast;
mod cache;
pub mod config;
pub mod generators;
mod impl_variant;
mod log;
mod preprocess;

pub async fn broadcast<Impl, F, E>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    spawn_generators: F,
) -> Result<(), E>
where
    F: FnOnce(TxRequestSender<Impl>) -> Result<SpawnResult, E>,
    Impl: impl_variant::Impl,
{
    let (tx_sender, tx_receiver): (
        UnboundedSender<TxRequest<Impl>>,
        UnboundedReceiver<TxRequest<Impl>>,
    ) = unbounded_channel();

    let SpawnResult {
        mut tx_generators_set,
        tx_result_senders,
    }: SpawnResult = spawn_generators(tx_sender)?;

    processing_loop(
        signer,
        config,
        node_client,
        node_config,
        tx_receiver,
        &mut tx_generators_set,
        tx_result_senders,
    )
    .await;

    tx_generators_set.shutdown().await;

    Ok(())
}

pub async fn poll_delivered_tx(
    node_client: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    hash: TxHash,
) -> Option<DeliverTx> {
    timeout(tick_time, async {
        loop {
            sleep(poll_time).await;

            match get_tx_response(node_client, hash).await {
                Ok(tx) => {
                    break tx;
                }
                Err(error) => {
                    error!(
                        hash = %hash,
                        error = ?error,
                        "Polling delivered transaction failed!",
                    );
                }
            }
        }
    })
    .await
    .ok()
}

pub(crate) struct ApiAndConfiguration {
    pub(crate) node_client: NodeClient,
    pub(crate) node_config: NodeConfig,
    pub(crate) signer: Signer,
    pub(crate) tick_time: Duration,
    pub(crate) poll_time: Duration,
}

#[inline]
async fn processing_loop<Impl>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    mut tx_receiver: UnboundedReceiver<TxRequest<Impl>>,
    tx_generators_set: &mut JoinSet<()>,
    mut tx_result_senders: BTreeMap<usize, CommitResultSender>,
) where
    Impl: impl_variant::Impl,
{
    let mut last_signing_timestamp: Instant = Instant::now();

    let mut api_and_configuration = ApiAndConfiguration {
        node_client,
        node_config,
        signer,
        tick_time: config.tick_time,
        poll_time: config.poll_time,
    };

    let mut next_sender_id: usize = 0;

    let mut requests_cache: cache::TxRequests<Impl> = cache::TxRequests::new();

    let mut preprocessed_tx_request: Option<preprocess::TxRequest<Impl>> = None;

    loop {
        if let Some(Err::<(), JoinError>(error)) =
            poll_fn(|cx| match tx_generators_set.poll_join_next(cx) {
                Poll::Pending => Poll::Ready(None),
                maybe_joined @ Poll::Ready(_) => maybe_joined,
            })
            .await
        {
            error!(
                "Generator task {}!",
                if error.is_panic() {
                    "panicked"
                } else if error.is_cancelled() {
                    "was cancelled"
                } else {
                    unreachable!()
                }
            );
        }

        if let Err(cache::ChannelClosed) =
            cache::purge_and_update(&mut tx_receiver, &mut requests_cache).await
        {
            info!("All generator threads stopped. Exiting.");

            return;
        }

        if preprocessed_tx_request.as_ref().map_or(
            true,
            |preprocess::TxRequest {
                 sender_id,
                 expiration,
                 ..
             }| {
                requests_cache.get_mut(sender_id).map_or_else(
                    || matches!(Impl::filter(expiration), FilterResult::Expired),
                    |slot| slot.get_mut().is_some(),
                )
            },
        ) {
            preprocessed_tx_request = preprocess::next_tx_request(
                &mut api_and_configuration,
                &mut requests_cache,
                &mut next_sender_id,
            )
            .await;
        }

        if let Some(tx_request) = preprocessed_tx_request.take() {
            match broadcast::sleep_and_broadcast_tx(
                &mut api_and_configuration,
                config.between_tx_margin_time,
                tx_request,
                &tx_result_senders,
                last_signing_timestamp,
            )
            .await
            {
                Ok(BroadcastAndSendBackTxHash {
                    broadcast_timestamp,
                    channel_closed,
                }) => {
                    last_signing_timestamp = broadcast_timestamp;

                    if let Some(ref sender_id) = channel_closed {
                        _ = tx_result_senders.remove(sender_id);

                        _ = requests_cache.remove(sender_id);
                    }
                }
                Err(tx_request) => {
                    info!("Placing transaction back in queue front to retry.");

                    preprocessed_tx_request = Some(tx_request);
                }
            }
        }
    }
}
