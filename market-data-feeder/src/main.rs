use std::{collections::BTreeMap, future::poll_fn, io, num::NonZeroU64, sync::Arc, task::Poll};

use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};
use serde::Deserialize;
use tokio::{
    select,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::{block_in_place, JoinSet},
    time::{sleep as tokio_sleep, sleep_until as tokio_sleep_until, Instant},
};
use tracing::{error, error_span, info, warn};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use chain_comms::{
    build_tx::ContractTx,
    client::Client as NodeClient,
    config::Node as NodeConfig,
    interact::{commit, get_tx_response, query},
    reexport::tonic::transport::Channel as TonicChannel,
    rpc_setup::{prepare_rpc, RpcSetup},
    signing_key::DEFAULT_COSMOS_HD_PATH,
};

use self::{config::Config, messages::QueryMsg, result::Result, workers::PriceDataPacket};

mod config;
mod deviation;
mod error;
mod log;
mod messages;
mod price;
mod provider;
mod providers;
mod result;
mod workers;

const COMPATIBLE_VERSION: SemVerComparator = SemVerComparator {
    op: semver::Op::GreaterEq,
    major: 0,
    minor: Some(5),
    patch: None,
    pre: SemVerPrerelease::EMPTY,
};

type UnboundedChannel<T> = (UnboundedSender<T>, UnboundedReceiver<T>);

#[tokio::main]
async fn main() -> Result<()> {
    let (log_writer, log_guard): (NonBlocking, non_blocking::WorkerGuard) =
        NonBlocking::new(rolling::hourly("./feeder-logs", "log"));

    chain_comms::log::setup(io::stdout.and(log_writer));

    info!(concat!(
        "Running version built on: ",
        env!("BUILD_START_TIME_DATE", "No build time provided!")
    ));

    let result: Result<()> = app_main().await;

    if let Err(error) = &result {
        error!(error = ?error, "{}", error);
    }

    drop(log_guard);

    result
}

async fn app_main() -> Result<()> {
    let RpcSetup {
        mut signer,
        config,
        node_client,
        ..
    }: RpcSetup<Config> = prepare_rpc("market-data-feeder.toml", DEFAULT_COSMOS_HD_PATH).await?;

    check_compatibility(&config, &node_client).await?;

    info!("Starting workers...");

    let workers::SpawnWorkersReturn {
        set: mut price_fetchers_set,
        id_to_name_mapping,
        receiver: mut price_data_receiver,
    }: workers::SpawnWorkersReturn = block_in_place(|| {
        workers::spawn(
            node_client.clone(),
            config.providers,
            config.comparison_providers,
            config.tick_time,
        )
    })?;

    info!("Entering broadcasting loop...");

    let node_config: Arc<NodeConfig> = Arc::new(config.node);

    let (retry_sender, mut retry_receiver): UnboundedChannel<PriceDataPacket> = unbounded_channel();

    let mut delivered_tx_fetchers_set: JoinSet<Option<NonZeroU64>> = JoinSet::new();

    let mut latest_timestamps: BTreeMap<usize, Instant> = BTreeMap::new();

    let mut fallback_gas_limit: Option<NonZeroU64> = None;

    let mut next_signing_timestamp = Instant::now();

    'outer_loop: loop {
        select! {
            Some(result) = price_fetchers_set.join_next(), if !price_fetchers_set.is_empty() => {
                match result {
                    Ok(Ok(())) => {},
                    Ok(Err(error)) => error!(
                        error = ?error,
                        "Provider exitted prematurely! Error: {error}",
                    ),
                    Err(error) => error!(
                        error = ?error,
                        "Provider exitted prematurely and was unable to be joined! Probable cause is a panic! Error: {error}",
                    ),
                }
            }
            Some(result) = delivered_tx_fetchers_set.join_next(), if !delivered_tx_fetchers_set.is_empty() => {
                match result {
                    Ok(Some(gas_used)) => fallback_gas_limit = Some(fallback_gas_limit.unwrap_or(gas_used).max(gas_used)),
                    Ok(None) => {}
                    Err(error) => error!(
                        error = ?error,
                        "Failure reported back from delivered transaction logger! Probable cause is a panic! Error: {error}",
                    ),
                }
            }
            Some(price_data_packet) = poll_fn(|cx| {
                if let Poll::Ready(packet) = price_data_receiver.poll_recv(cx) {
                    if packet.is_some() {
                        return Poll::Ready(packet);
                    }

                    retry_receiver.close();
                }

                retry_receiver.poll_recv(cx)
            }) => {
                let PriceDataPacket {
                    provider_id,
                    tx_time,
                    ..
                } = price_data_packet;

                if tx_time.elapsed() >= config.tick_time {
                    warn!(
                        provider = id_to_name_mapping[&provider_id].as_ref(),
                        "Transaction data timed out."
                    );

                    continue;
                }

                let saved_timestamp = latest_timestamps.entry(provider_id).or_insert(tx_time);

                if *saved_timestamp < tx_time {
                    *saved_timestamp = tx_time;
                } else if *saved_timestamp != tx_time {
                    warn!(
                        provider = id_to_name_mapping[&provider_id].as_ref(),
                        "Transaction data already superceded."
                    );

                    continue;
                }

                if next_signing_timestamp.elapsed() < config.between_tx_margin_time {
                    tokio_sleep_until(next_signing_timestamp).await;
                }

                let result = commit::with_gas_estimation(
                    &mut signer,
                    &node_client,
                    &node_config,
                    config.hard_gas_limit,
                    fallback_gas_limit.unwrap_or(config.hard_gas_limit),
                    ContractTx::new(price_data_packet.message.oracle.as_ref().into()).add_message(
                        price_data_packet.message.execute_message.to_vec(),
                        Vec::new(),
                    ),
                )
                .await;

                next_signing_timestamp = Instant::now() + config.between_tx_margin_time;

                match result {
                    Ok(response) => {
                        log::commit_response(&id_to_name_mapping[&provider_id], &response);

                        let hash = response.hash;
                        let node_client = node_client.clone();
                        let retry_sender = retry_sender.clone();
                        let provider_id = id_to_name_mapping[&provider_id].clone();

                        delivered_tx_fetchers_set.spawn(async move {
                            tokio_sleep(config.retry_tick_time).await;

                            match get_tx_response(&node_client, hash).await {
                                Ok(response) => {
                                    log::tx_response(&{ provider_id }, &hash, &response);

                                    NonZeroU64::new(response.gas_used.unsigned_abs())
                                        .map(|gas_used| {
                                            gas_used.min(config.hard_gas_limit)
                                        })
                                }
                                Err(error) => {
                                    error_span!(
                                        "Delivered Tx",
                                        provider_id = { provider_id }.as_ref(),
                                    )
                                    .in_scope(|| {
                                        info!("Hash: {hash}");

                                        error!(
                                            error = ?error,
                                            "Failed to fetch transaction response! Cause: {error}",
                                        );

                                        info!("Sending transaction for retry.");

                                        if retry_sender.send(price_data_packet).is_err() {
                                            warn!("Sending for retry failed. Channel is closed.");
                                        }
                                    });

                                    None
                                }
                            }
                        });
                    }
                    Err(error) => error!(
                        error = ?error,
                        "Failed to feed data into oracle! Cause: {error}",
                    ),
                }
            }
            else => {
                warn!("Transaction receiving channel is closed!");

                retry_receiver.close();

                break 'outer_loop;
            }
        }
    }

    assert!(price_fetchers_set.is_empty());
    assert!(delivered_tx_fetchers_set.is_empty());

    Ok(())
}

async fn check_compatibility(config: &Config, node_client: &NodeClient) -> Result<()> {
    #[derive(Deserialize)]
    struct JsonVersion {
        major: u64,
        minor: u64,
        patch: u64,
    }

    info!("Checking compatibility with contract version...");

    for (oracle_name, oracle_address) in &config.oracles {
        let version: JsonVersion = node_client
            .with_grpc(|rpc: TonicChannel| {
                query::wasm(rpc, oracle_address.to_string(), QueryMsg::CONTRACT_VERSION)
            })
            .await?;

        let version: Version = Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            pre: SemVerPrerelease::EMPTY,
            build: SemVerBuildMetadata::EMPTY,
        };

        if !COMPATIBLE_VERSION.matches(&version) {
            error!(
                oracle = %oracle_name,
                compatible = %COMPATIBLE_VERSION,
                actual = %version,
                "Feeder version is incompatible with contract version!"
            );

            return Err(error::Application::IncompatibleContractVersion {
                oracle_addr: oracle_address.clone(),
                compatible: COMPATIBLE_VERSION,
                actual: version,
            });
        }
    }

    info!("Contract is compatible with feeder version.");

    Ok(())
}
