#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_errors_doc,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening
)]

use std::{
    collections::btree_map::BTreeMap, error::Error, future::poll_fn, task::Poll, time::Duration,
};

use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::{sleep, timeout, Instant},
};
use tracing::{error, info};

use chain_comms::{
    client::Client as NodeClient,
    config::Node as NodeConfig,
    interact::{error::GetTxResponse as GetTxResponseError, get_tx_response},
    reexport::cosmrs::tendermint::{abci::response::DeliverTx, Hash as TxHash},
    signer::Signer,
};

use self::{
    broadcast::ProcessingOutput as BroadcastProcessingOutput,
    config::Config,
    generators::{CommitResultSender, SpawnResult, TxRequest, TxRequestSender},
    mode::FilterResult,
};

mod broadcast;
mod cache;
pub mod config;
pub mod generators;
pub mod log;
pub mod mode;
mod preprocess;

#[allow(clippy::future_not_send)]
pub async fn broadcast<Impl, GeneratorError, SpawnGeneratorsF, SpawnE>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    spawn_generators: SpawnGeneratorsF,
) -> Result<(), SpawnE>
where
    Impl: mode::Impl,
    SpawnGeneratorsF:
        FnOnce(TxRequestSender<Impl>) -> Result<SpawnResult<GeneratorError>, SpawnE> + Send,
    GeneratorError: Error + Send + 'static,
{
    let (tx_sender, tx_receiver): (
        UnboundedSender<TxRequest<Impl>>,
        UnboundedReceiver<TxRequest<Impl>>,
    ) = unbounded_channel();

    let SpawnResult {
        mut tx_generators_set,
        tx_result_senders,
    }: SpawnResult<GeneratorError> = spawn_generators(tx_sender)?;

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

            let result: Result<DeliverTx, GetTxResponseError> =
                get_tx_response(node_client, hash).await;

            match result {
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
#[allow(clippy::future_not_send)]
async fn processing_loop<Impl, GeneratorError>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    mut tx_receiver: UnboundedReceiver<TxRequest<Impl>>,
    tx_generators_set: &mut JoinSet<Result<(), GeneratorError>>,
    mut tx_result_senders: BTreeMap<usize, CommitResultSender>,
) where
    Impl: mode::Impl,
    GeneratorError: Error + Send + 'static,
{
    let mut last_signing_timestamp: Instant = Instant::now();

    let mut next_sender_id: usize = 0;

    let mut requests_cache: cache::TxRequests<Impl> = cache::TxRequests::new();

    let mut preprocessed_tx_request: Option<preprocess::TxRequest<Impl>> = None;

    let mut api_and_configuration = ApiAndConfiguration {
        node_client,
        node_config,
        signer,
        tick_time: config.tick_time,
        poll_time: config.poll_time,
    };

    loop {
        try_join_generator_task(tx_generators_set).await;

        if matches!(
            cache::purge_and_update(&mut tx_receiver, &mut requests_cache).await,
            Err(cache::ChannelClosed {})
        ) {
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
                &requests_cache,
                &mut next_sender_id,
            )
            .await;
        }

        if let Some(tx_request) = preprocessed_tx_request.take() {
            let broadcast_result: Result<BroadcastProcessingOutput, preprocess::TxRequest<Impl>> =
                broadcast::sleep_and_broadcast_tx(
                    &mut api_and_configuration,
                    config.between_tx_margin_time,
                    tx_request,
                    &tx_result_senders,
                    last_signing_timestamp,
                )
                .await;

            match broadcast_result {
                Ok(BroadcastProcessingOutput {
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

#[allow(clippy::needless_pass_by_ref_mut)]
async fn try_join_generator_task<GeneratorError>(
    tx_generators_set: &mut JoinSet<Result<(), GeneratorError>>,
) where
    GeneratorError: Error + Send + 'static,
{
    if let Some(result) = poll_fn(move |cx| match tx_generators_set.poll_join_next(cx) {
        Poll::Pending => Poll::Ready(None),
        maybe_joined @ Poll::Ready(_) => maybe_joined,
    })
    .await
    {
        match result {
            Ok(Ok(())) => {
                info!("Generator task exited without errors.");
            }
            Ok(Err(error)) => {
                error!(error = ?error, "Generator task exited with error! Error: {error}");
            }
            Err(error) => {
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
        }
    }
}
