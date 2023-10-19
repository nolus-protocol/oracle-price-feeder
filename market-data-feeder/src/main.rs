use std::{
    collections::BTreeMap,
    io,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::future::poll_fn;
use serde::Deserialize;
use tokio::{
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        watch,
    },
    task::JoinSet,
    time::{sleep, timeout, Instant},
};
use tracing::{error, info, info_span, warn};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use chain_comms::{
    build_tx::ContractTx,
    client::Client as NodeClient,
    interact::{
        commit_tx_with_gas_estimation, error::GasEstimatingTxCommit, query_wasm, CommitResponse,
    },
    log,
    reexport::tonic::transport::Channel as TonicChannel,
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
    signing_key::DEFAULT_COSMOS_HD_PATH,
};
use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};

use self::{config::Config, messages::QueryMsg, result::Result};

mod config;
mod deviation;
mod error;
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
type WatchChannel<T> = (watch::Sender<T>, watch::Receiver<T>);

#[tokio::main]
async fn main() -> Result<()> {
    let (log_writer, log_guard): (NonBlocking, non_blocking::WorkerGuard) =
        NonBlocking::new(rolling::hourly("./feeder-logs", "log"));

    log::setup(io::stdout.and(log_writer));

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
        nolus_node,
        ..
    }: RpcSetup<Config> = prepare_rpc("market-data-feeder.toml", DEFAULT_COSMOS_HD_PATH).await?;

    check_compatibility(&config, &nolus_node).await?;

    info!("Starting workers...");

    let tick_time: Duration = Duration::from_secs(config.tick_time);

    let (recovery_mode_sender, recovery_mode_receiver): WatchChannel<bool> = watch::channel(false);

    let workers::SpawnWorkersReturn {
        set: mut price_fetchers_set,
        receivers,
    }: workers::SpawnWorkersReturn = workers::spawn(
        nolus_node.clone(),
        config.oracles,
        config.providers,
        config.comparison_providers,
        tick_time,
        recovery_mode_receiver,
    )
    .await?;

    info!("Entering broadcasting loop...");

    let mut price_feeders_set: JoinSet<()> = JoinSet::new();

    let node_config = Arc::new(config.node);

    let (tx_sender, mut tx_receiver): UnboundedChannel<(Instant, ContractTx)> = unbounded_channel();

    for (oracle, price_data_receiver) in receivers {
        price_feeders_set.spawn(price_feeder(
            oracle,
            tick_time,
            price_data_receiver,
            tx_sender.clone(),
        ));
    }

    drop(tx_sender);

    let mut fallback_gas_limit: Option<u64> = None;

    'outer_loop: while !price_fetchers_set.is_empty() && !price_feeders_set.is_empty() {
        if let Some(result) =
            poll_fn(
                |cx: &mut Context| match price_fetchers_set.poll_join_next(cx) {
                    Poll::Pending => Poll::Ready(None),
                    result => result,
                },
            )
            .await
        {
            match result {
                Ok(Ok(())) => unreachable!(),
                Ok(Err(error)) => {
                    error!(error = ?error, "Provider exitted prematurely! Error: {error}", error = error)
                }
                Err(error) => {
                    error!(error = ?error, "Provider exitted prematurely and was unable to be joined! Probable cause is a panic! Error: {error}", error = error)
                }
            }
        }

        if let Some(result) =
            poll_fn(
                |cx: &mut Context| match price_feeders_set.poll_join_next(cx) {
                    Poll::Pending => Poll::Ready(None),
                    result => result,
                },
            )
            .await
        {
            match result {
                Ok(()) => info!("Oracle feeder task exited"),
                Err(error) => {
                    error!(error = ?error, "Provider exitted prematurely and was unable to be joined! Probable cause is a panic! Error: {error}", error = error)
                }
            }
        }

        let Some((tx_time, tx)): Option<(Instant, ContractTx)> = tx_receiver.recv().await else {
            break;
        };

        let mut tx: Option<ContractTx> = Some(tx);

        let mut is_retry: bool = false;

        while let Some(tx) = if is_retry {
            if tx_time.elapsed() < tick_time {
                tx.take()
            } else {
                None
            }
        } else {
            is_retry = true;

            tx.clone()
        } {
            let successful: bool = commit_tx_with_gas_estimation(
                &mut signer,
                &nolus_node,
                &node_config,
                config.gas_limit,
                tx,
                fallback_gas_limit,
            )
            .await
            .map_or_else(
                |error: GasEstimatingTxCommit| {
                    error!("Failed to feed data into oracle! Cause: {error}");

                    false
                },
                |response: CommitResponse| {
                    log::commit_response(&response);

                    if response.check_tx.code.is_ok() && response.deliver_tx.code.is_ok() {
                        let used_gas: u64 = response.deliver_tx.gas_used.unsigned_abs();

                        let fallback_gas_limit: &mut u64 =
                            fallback_gas_limit.get_or_insert(used_gas);

                        *fallback_gas_limit = used_gas.max(*fallback_gas_limit);

                        true
                    } else {
                        false
                    }
                },
            );

            if successful {
                continue 'outer_loop;
            } else if signer.needs_update()
                && recovery_loop(&mut signer, &recovery_mode_sender, &nolus_node)
                    .await
                    .is_error()
            {
                break 'outer_loop;
            }
        }
    }

    Ok(())
}

async fn price_feeder(
    oracle: Arc<str>,
    tick_time: Duration,
    mut price_data_receiver: UnboundedReceiver<(usize, Instant, Vec<u8>)>,
    tx_sender: UnboundedSender<(Instant, ContractTx)>,
) {
    loop {
        let mut messages: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

        let channel_closed: bool = timeout(tick_time, async {
            while let Some((id, instant, data)) = price_data_receiver.recv().await {
                if Instant::now().duration_since(instant) < tick_time {
                    messages.insert(id, data);
                }
            }

            true
        })
        .await
        .unwrap_or_default();

        if messages.is_empty() {
            if channel_closed {
                break;
            } else {
                continue;
            }
        }

        let tx: ContractTx = messages.into_values().fold(
            ContractTx::new(oracle.to_string()),
            |tx: ContractTx, msg: Vec<u8>| {
                error!(tx = %String::from_utf8_lossy(&msg));

                tx.add_message(msg, Vec::new())
            },
        );

        if tx_sender.send((Instant::now(), tx)).is_err() {
            break;
        }
    }
}

enum RecoveryStatus {
    Success,
    Error,
}

impl RecoveryStatus {
    const fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }
}

async fn recovery_loop(
    signer: &mut Signer,
    recovery_mode_sender: &watch::Sender<bool>,
    client: &NodeClient,
) -> RecoveryStatus {
    let set_in_recovery = info_span!("recover-after-error").in_scope(|| {
        info!("After-error recovery needed!");

        |in_recovery: bool| {
            let is_error: bool = recovery_mode_sender.send(in_recovery).is_err();

            if is_error {
                error!("Recovery mode state watch closed! Exiting broadcasting loop...");
            }

            is_error
        }
    });

    let recovered: RecoveryStatus = recover_after_error(signer, client).await;

    if recovered.is_error() {
        if set_in_recovery(true) {
            return RecoveryStatus::Error;
        }

        while recover_after_error(signer, client).await.is_error() {
            sleep(Duration::from_secs(15)).await;
        }

        if set_in_recovery(false) {
            return RecoveryStatus::Error;
        }
    }

    RecoveryStatus::Success
}

async fn check_compatibility(config: &Config, client: &NodeClient) -> Result<()> {
    #[derive(Deserialize)]
    struct JsonVersion {
        major: u64,
        minor: u64,
        patch: u64,
    }

    info!("Checking compatibility with contract version...");

    for oracle in config.oracles.iter() {
        let version: JsonVersion = client
            .with_grpc(|rpc: TonicChannel| {
                query_wasm(rpc, oracle.to_string(), QueryMsg::CONTRACT_VERSION)
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
                oracle = %oracle,
                compatible = %COMPATIBLE_VERSION,
                actual = %version,
                "Feeder version is incompatible with contract version!"
            );

            return Err(error::Application::IncompatibleContractVersion {
                oracle_addr: oracle.clone(),
                compatible: COMPATIBLE_VERSION,
                actual: version,
            });
        }
    }

    info!("Contract is compatible with feeder version.");

    Ok(())
}

#[must_use]
async fn recover_after_error(signer: &mut Signer, client: &NodeClient) -> RecoveryStatus {
    if let Err(error) = signer.update_account(client).await {
        error!("{error}");

        return RecoveryStatus::Error;
    }

    info!("Successfully updated local copy of account data.");

    info!("Continuing normal workflow...");

    RecoveryStatus::Success
}
