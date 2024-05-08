#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_errors_doc,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening
)]

use std::{
    collections::btree_map::BTreeMap,
    convert::Infallible,
    future::{poll_fn, Future},
    pin::pin,
    task::Poll,
    time::Duration,
};

use tokio::{
    select,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::{sleep, timeout, Instant},
};
use tracing::{error, info};

use chain_comms::{
    client::Client as NodeClient,
    config::Node as NodeConfig,
    interact::{
        get_tx_response::{
            error::Error as GetTxResponseError, get_tx_response,
            Response as TxResponse,
        },
        healthcheck::Healthcheck,
        TxHash,
    },
    signer::Signer,
};

use crate::broadcast::ProcessingError;

use self::{
    broadcast::{
        ProcessingError as BroadcastProcessingError,
        ProcessingOutput as BroadcastProcessingOutput,
    },
    config::Config,
    generators::{CommitResultSender, SpawnResult, TxRequest, TxRequestSender},
    mode::FilterResult,
};

mod broadcast;
mod cache;
pub mod config;
pub mod error;
pub mod generators;
pub mod log;
pub mod mode;
mod preprocess;

#[allow(clippy::future_not_send)]
pub async fn broadcast<Impl, SpawnGeneratorsF, SpawnGeneratorsFuture, SpawnE>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    spawn_generators: SpawnGeneratorsF,
) -> Result<(), error::Error<SpawnE>>
where
    Impl: mode::Impl,
    SpawnGeneratorsF:
        FnOnce(TxRequestSender<Impl>) -> SpawnGeneratorsFuture + Send,
    SpawnGeneratorsFuture: Future<Output = Result<SpawnResult, SpawnE>> + Send,
    SpawnE: std::error::Error,
{
    let (tx_sender, tx_receiver): (
        UnboundedSender<TxRequest<Impl>>,
        UnboundedReceiver<TxRequest<Impl>>,
    ) = unbounded_channel();

    let SpawnResult {
        mut tx_generators_set,
        tx_result_senders,
    }: SpawnResult = spawn_generators(tx_sender)
        .await
        .map_err(error::Error::SpawnGenerators)?;

    let mut signal = pin!(tokio::signal::ctrl_c());

    let signal_installed: bool = if let Err(error) =
        poll_fn(|cx| match signal.as_mut().poll(cx) {
            result @ Poll::Ready(_) => result,
            Poll::Pending => Poll::Ready(Ok(())),
        })
        .await
    {
        error!(
            ?error,
            "Failed to install Ctrl+C signal handler! Cause: {error}"
        );

        false
    } else {
        true
    };

    let result = select! {
        result = signal, if signal_installed => {
            match result {
                Ok(()) => {
                    info!("Received Ctrl+C signal. Stopping task.");
                }
                Err(error) => {
                    error!(?error, "Error received from Ctrl+C signal handler! Stopping task! Error: {error}");
                }
            }

            Ok(())
        },
        result = processing_loop(
            signer,
            config,
            node_client,
            node_config,
            tx_receiver,
            &mut tx_generators_set,
            tx_result_senders,
        ) => result,
    };

    tx_generators_set.shutdown().await;

    result
}

pub async fn poll_delivered_tx(
    node_client: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    tx_hash: TxHash,
) -> Option<TxResponse> {
    timeout(tick_time, async {
        loop {
            sleep(poll_time).await;

            let result: Result<TxResponse, GetTxResponseError> =
                get_tx_response(node_client, tx_hash.0.clone()).await;

            match result {
                Ok(tx) => {
                    break tx;
                },
                Err(error) => {
                    error!(
                        hash = %tx_hash,
                        error = ?error,
                        "Polling delivered transaction failed!",
                    );
                },
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
async fn processing_loop<Impl, E>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    mut tx_receiver: UnboundedReceiver<TxRequest<Impl>>,
    tx_generators_set: &mut JoinSet<Infallible>,
    mut tx_result_senders: BTreeMap<usize, CommitResultSender>,
) -> Result<(), error::Error<E>>
where
    Impl: mode::Impl,
    E: std::error::Error,
{
    let mut last_signing_timestamp: Instant = Instant::now();

    let mut next_sender_id: usize = 0;

    let mut requests_cache: cache::TxRequests<Impl> = cache::TxRequests::new();

    let mut preprocessed_tx_request: Option<preprocess::TxRequest<Impl>> = None;

    let mut healthcheck =
        Healthcheck::new(node_client.tendermint_service_client()).await?;

    let mut api_and_configuration = ApiAndConfiguration {
        node_client,
        node_config,
        signer,
        tick_time: config.tick_time(),
        poll_time: config.poll_time(),
    };

    let mut sequence_mismatch_streak_first_timestamp = None;

    loop {
        if let Err::<(), _>(error) = Impl::healthcheck(&mut healthcheck).await {
            error!(
                ?error,
                "Healthcheck failed due to an error! Error: {error}",
            );

            continue;
        }

        try_join_generator_task(tx_generators_set).await;

        if matches!(
            cache::purge_and_update(&mut tx_receiver, &mut requests_cache)
                .await,
            Err(cache::ChannelClosed {})
        ) {
            info!("All generator threads stopped. Exiting.");

            return Ok(());
        }

        if preprocessed_tx_request.as_ref().map_or(
            true,
            |preprocess::TxRequest {
                 sender_id,
                 expiration,
                 ..
             }| {
                requests_cache.get_mut(sender_id).map_or_else(
                    || {
                        matches!(
                            Impl::filter(expiration),
                            FilterResult::Expired
                        )
                    },
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
            let broadcast_result: Result<
                BroadcastProcessingOutput,
                preprocess::TxRequest<Impl>,
            > = broadcast::sleep_and_broadcast_tx(
                &mut api_and_configuration,
                config.between_tx_margin_time(),
                tx_request,
                &tx_result_senders,
                last_signing_timestamp,
            )
            .await;

            match broadcast_result {
                Ok(BroadcastProcessingOutput {
                    broadcast_timestamp,
                    error,
                    channel_closed,
                }) => {
                    last_signing_timestamp = broadcast_timestamp;

                    if let Some(error) = error {
                        handle_mempool_error(
                            &mut api_and_configuration,
                            &mut sequence_mismatch_streak_first_timestamp,
                            broadcast_timestamp,
                            config.tick_time(),
                            error,
                        )
                        .await;
                    } else {
                        sequence_mismatch_streak_first_timestamp = None;
                    }

                    if let Some(ref sender_id) = channel_closed {
                        _ = tx_result_senders.remove(sender_id);

                        _ = requests_cache.remove(sender_id);
                    }
                },
                Err(tx_request) => {
                    info!("Placing transaction back in queue front to retry.");

                    preprocessed_tx_request = Some(tx_request);
                },
            }
        }
    }
}

async fn handle_mempool_error(
    api_and_configuration: &mut ApiAndConfiguration,
    sequence_mismatch_streak_first_timestamp: &mut Option<Instant>,
    broadcast_timestamp: Instant,
    tick_time: Duration,
    error: ProcessingError,
) {
    match error {
        BroadcastProcessingError::VerificationFailed => {
            if let Err(error) = api_and_configuration
                .signer
                .fetch_chain_id(&api_and_configuration.node_client)
                .await
            {
                error!(%error, "Failed to re-fetch chain ID! Cause: {error}");
            } else {
                info!("Successfully re-fetched chain ID.");
            }
        },
        BroadcastProcessingError::SequenceMismatch => {
            if sequence_mismatch_streak_first_timestamp
                .get_or_insert(broadcast_timestamp)
                .elapsed()
                >= tick_time
            {
                if let Err(error) = api_and_configuration
                    .signer
                    .fetch_sequence_number(&api_and_configuration.node_client)
                    .await
                {
                    error!(%error, "Failed to re-fetch account data! Cause: {error}");
                } else {
                    info!("Successfully re-fetched account data.");
                }
            }
        },
    }
}

#[allow(clippy::needless_pass_by_ref_mut)]
async fn try_join_generator_task(tx_generators_set: &mut JoinSet<Infallible>) {
    if let Some(error) =
        poll_fn(move |cx| match tx_generators_set.poll_join_next(cx) {
            Poll::Pending => Poll::Ready(None),
            Poll::Ready(maybe_joined) => {
                Poll::Ready(maybe_joined.and_then(Result::err))
            },
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
}
