use std::{
    collections::BTreeMap,
    io,
    result::Result as StdResult,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use futures::future::poll_fn;
use tokio::{
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        watch,
    },
    time::{error::Elapsed, sleep, timeout, Instant},
};
use tracing::{error, info, info_span};
use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::RollingFileAppender,
};

use chain_comms::{
    build_tx::ContractTx,
    client::Client,
    interact::{
        commit_tx_with_gas_estimation, error::GasEstimatingTxCommit, query_wasm, CommitResponse,
    },
    log,
    reexport::tonic::transport::Channel as TonicChannel,
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
    signing_key::DEFAULT_COSMOS_HD_PATH,
};
use semver::SemVer;

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

const COMPATIBLE_VERSION: SemVer = SemVer::new(0, 5, 0);

type UnboundedChannel<T> = (UnboundedSender<T>, UnboundedReceiver<T>);
type WatchChannel<T> = (watch::Sender<T>, watch::Receiver<T>);

#[tokio::main]
async fn main() -> Result<()> {
    static LOGGER_ON_DROP: AtomicBool = AtomicBool::new(false);

    let log_writer: RollingFileAppender = tracing_appender::rolling::hourly("./feeder-logs", "log");

    let (log_writer, log_guard): (NonBlocking, WorkerGuard) = tracing_appender::non_blocking(
        log::CombinedWriter::new(io::stdout(), log_writer, &LOGGER_ON_DROP),
    );

    log::setup(log_writer)?;

    info!(concat!(
        "Running version built on: ",
        env!("BUILD_START_TIME_DATE", "No build time provided!")
    ));

    let result: Result<()> = app_main().await;

    if let Err(error) = &result {
        error!(error = ?error, "{}", error);
    }

    drop(log_guard);

    while !LOGGER_ON_DROP.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }

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

    let nolus_node: Arc<Client> = Arc::new(nolus_node);

    info!("Starting workers...");

    let tick_time: Duration = Duration::from_secs(config.tick_time);

    let (recovery_mode_sender, recovery_mode_receiver): WatchChannel<bool> = watch::channel(false);

    let workers::SpawnWorkersReturn {
        mut set,
        mut receiver,
    }: workers::SpawnWorkersReturn = workers::spawn(
        nolus_node.clone(),
        config.providers,
        config.comparison_providers,
        config.oracle_addr.clone(),
        tick_time,
        recovery_mode_receiver,
    )
    .await?;

    info!("Entering broadcasting loop...");

    let mut fallback_gas_limit: Option<u64> = None;

    'feeder_loop: while !set.is_empty() {
        if let Some(result) = poll_fn(|cx: &mut Context| match set.poll_join_next(cx) {
            Poll::Pending => Poll::Ready(None),
            result => result,
        })
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

        let mut messages: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

        let _: StdResult<(), Elapsed> = timeout(tick_time, async {
            while let Some((id, instant, data)) = receiver.recv().await {
                if Instant::now().duration_since(instant) < tick_time {
                    messages.insert(id, Vec::from(data));
                }
            }
        })
        .await;

        if messages.is_empty() {
            continue 'feeder_loop;
        }

        let mut is_retry: bool = false;

        let mut tx: Option<ContractTx> = Some(messages.into_values().fold(
            ContractTx::new(config.oracle_addr.to_string()),
            |tx: ContractTx, msg: Vec<u8>| tx.add_message(msg, Vec::new()),
        ));

        let first_try_timestamp: Instant = Instant::now();

        while let Some(tx) = if is_retry {
            if first_try_timestamp.elapsed() < tick_time {
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
                &config.node,
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
                continue 'feeder_loop;
            } else if signer.needs_update()
                && recovery_loop(&mut signer, &recovery_mode_sender, &nolus_node)
                    .await
                    .is_error()
            {
                break 'feeder_loop;
            }
        }
    }

    drop(receiver);

    Ok(())
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
    client: &Arc<Client>,
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

    let recovered: RecoveryStatus = recover_after_error(signer, client.as_ref()).await;

    if recovered.is_error() {
        if set_in_recovery(true) {
            return RecoveryStatus::Error;
        }

        while recover_after_error(signer, client.as_ref())
            .await
            .is_error()
        {
            sleep(Duration::from_secs(15)).await;
        }

        if set_in_recovery(false) {
            return RecoveryStatus::Error;
        }
    }

    RecoveryStatus::Success
}

async fn check_compatibility(config: &Config, client: &Client) -> Result<()> {
    info!("Checking compatibility with contract version...");

    {
        let version: SemVer = client
            .with_grpc(|rpc: TonicChannel| {
                query_wasm(rpc, &config.oracle_addr, QueryMsg::CONTRACT_VERSION)
            })
            .await?;

        if !version.check_compatibility(COMPATIBLE_VERSION) {
            error!(
                compatible_minimum = %COMPATIBLE_VERSION,
                actual = %version,
                "Feeder version is incompatible with contract version!"
            );

            return Err(error::Application::IncompatibleContractVersion {
                minimum_compatible: COMPATIBLE_VERSION,
                actual: version,
            });
        }
    }

    info!("Contract is compatible with feeder version.");

    Ok(())
}

#[must_use]
async fn recover_after_error(signer: &mut Signer, client: &Client) -> RecoveryStatus {
    if let Err(error) = signer.update_account(client).await {
        error!("{error}");

        return RecoveryStatus::Error;
    }

    info!("Successfully updated local copy of account data.");

    info!("Continuing normal workflow...");

    RecoveryStatus::Success
}
