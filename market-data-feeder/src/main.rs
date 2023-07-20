use std::{collections::BTreeMap, io, str::FromStr, sync::Arc, time::Duration};

use tokio::{
    select,
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver},
        watch,
    },
    task::JoinSet,
    time::{interval, sleep, timeout, Instant},
};
use tracing::{error, info, info_span, span::EnteredSpan};

use chain_comms::{
    build_tx::ContractTx,
    client::Client,
    interact::{commit_tx_with_gas_estimation, query_wasm, CommitResponse},
    log::{self, log_commit_response, setup_logging},
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
};
use semver::SemVer;

use self::{
    config::{Config, Provider as ProviderConfig},
    error::AppResult,
    messages::{ExecuteMsg, QueryMsg},
    provider::{Factory, Provider, Type},
};

pub mod config;
pub mod error;
pub mod messages;
pub mod provider;

pub const COMPATIBLE_VERSION: SemVer = SemVer::new(0, 5, 0);

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_SEQ_ERRORS: u8 = 5;

pub const MAX_SEQ_ERRORS_SLEEP_DURATION: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> AppResult<()> {
    let log_writer = tracing_appender::rolling::hourly("./feeder-logs", "log");

    let (log_writer, _guard) =
        tracing_appender::non_blocking(log::CombinedWriter::new(io::stdout(), log_writer));

    setup_logging(log_writer)?;

    info!(concat!(
        "Running version built on: ",
        env!("BUILD_START_TIME_DATE", "No build time provided!")
    ));

    let result: AppResult<()> = app_main().await;

    if let Err(error) = &result {
        error!("{error}");
    }

    result
}

async fn app_main() -> AppResult<()> {
    let RpcSetup {
        mut signer,
        config,
        client,
        ..
    } = prepare_rpc::<Config, _>("market-data-feeder.toml", DEFAULT_COSMOS_HD_PATH).await?;

    check_compatibility(&config, &client).await?;

    let client = Arc::new(client);

    info!("Starting workers...");

    let tick_time = Duration::from_secs(config.tick_time());

    let (recovery_mode_sender, recovery_mode_receiver): (
        watch::Sender<bool>,
        watch::Receiver<bool>,
    ) = watch::channel(false);

    let (mut set, mut receiver) = spawn_workers(
        &client,
        config.providers(),
        config.oracle_addr(),
        tick_time,
        recovery_mode_receiver,
    )?;

    info!("Workers started. Entering broadcasting loop...");

    let mut fallback_gas_limit: Option<u64> = None;

    'feeder_loop: loop {
        let mut messages: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

        let channel_closed: bool = timeout(tick_time, async {
            while let Some((id, instant, data)) = receiver.recv().await {
                if Instant::now().duration_since(instant) < tick_time {
                    messages.insert(id, Vec::from(data));
                }
            }

            true
        })
        .await
        .unwrap_or(false);

        if messages.is_empty() {
            if channel_closed {
                break 'feeder_loop;
            } else {
                continue 'feeder_loop;
            }
        }

        let mut is_retry: bool = false;

        let mut tx: Option<ContractTx> = Some(messages.into_values().fold(
            ContractTx::new(config.oracle_addr().to_string()),
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
            let result: Result<
                CommitResponse,
                chain_comms::interact::error::GasEstimatingTxCommit,
            > = commit_tx_with_gas_estimation(
                &mut signer,
                &client,
                config.as_ref(),
                config.gas_limit(),
                tx,
                fallback_gas_limit,
            )
            .await;

            match result {
                Ok(response) => {
                    log_commit_response(&response);

                    if response.check_tx.code.is_ok() && response.deliver_tx.code.is_ok() {
                        let used_gas: u64 = response.deliver_tx.gas_used.unsigned_abs();

                        let fallback_gas_limit: &mut u64 =
                            fallback_gas_limit.get_or_insert(used_gas);

                        *fallback_gas_limit = used_gas.max(*fallback_gas_limit);

                        continue 'feeder_loop;
                    }
                }
                Err(error) => error!("Failed to feed data into oracle! Cause: {error}"),
            }

            if signer.needs_update() {
                let set_in_recovery = |in_recovery: bool| {
                    let is_error: bool = recovery_mode_sender.send(in_recovery).is_err();

                    if is_error {
                        error!("Recovery mode state watch closed! Exiting broadcasting loop...");
                    }

                    is_error
                };

                let recovered: bool =
                    recover_after_error(&mut signer, client.as_ref(), tick_time, &mut receiver)
                        .await;

                if !recovered {
                    if set_in_recovery(true) {
                        break 'feeder_loop;
                    }

                    while !recover_after_error(
                        &mut signer,
                        client.as_ref(),
                        tick_time,
                        &mut receiver,
                    )
                    .await
                    {
                        sleep(Duration::from_secs(15)).await;
                    }

                    if set_in_recovery(false) {
                        break 'feeder_loop;
                    }
                }
            }
        }
    }

    drop(receiver);

    while set.join_next().await.is_some() {}

    Ok(())
}

async fn check_compatibility(config: &Config, client: &Client) -> AppResult<()> {
    info!("Checking compatibility with contract version...");

    {
        let version: SemVer = query_wasm(
            client,
            config.oracle_addr(),
            &serde_json_wasm::to_vec(&QueryMsg::ContractVersion {})?,
        )
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
async fn recover_after_error(
    signer: &mut Signer,
    client: &Client,
    tick_time: Duration,
    receiver: &mut UnboundedReceiver<(usize, Instant, String)>,
) -> bool {
    let mut channel_closed: bool = false;

    let _span: EnteredSpan = info_span!("recover-after-error").entered();

    info!("After-error recovery needed!");

    'recovery_loop: while signer.needs_update() {
        info!("Trying to update local copy of account data...");

        let result: Result<(), chain_comms::interact::error::AccountQuery> = select!(
            update_result = signer.update_account(client) => update_result,
            () = async {
                while receiver.recv().await.is_some() {}

                channel_closed = true;

                std::future::pending().await
            } => unreachable!(),
        );

        if let Err(error) = result {
            error!("Failed to update local copy of account data! Cause: {error}");

            sleep(tick_time).await;
        } else {
            break 'recovery_loop;
        }
    }

    info!("Successfully updated local copy of account data.");

    info!("Continuing workflow.");

    drop(_span);

    channel_closed
}

type SpawnWorkersResult = AppResult<(
    JoinSet<Result<(), error::Worker>>,
    UnboundedReceiver<(usize, Instant, String)>,
)>;

fn spawn_workers(
    client: &Arc<Client>,
    providers: &[ProviderConfig],
    oracle_addr: &str,
    tick_time: Duration,
    recovery_mode: watch::Receiver<bool>,
) -> SpawnWorkersResult {
    let mut set = JoinSet::new();

    let (sender, receiver) = unbounded_channel();

    providers
        .iter()
        .map(|provider_cfg| {
            let p_type = Type::from_str(&provider_cfg.main_type)?;

            Factory::new_provider(&p_type, provider_cfg)
                .map_err(error::Application::InstantiateProvider)
        })
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .enumerate()
        .for_each(|(monotonic_id, provider)| {
            let client = client.clone();

            let sender = sender.clone();

            let provider_name = format!("Provider #{}/\"{}\"", monotonic_id, provider.name());

            set.spawn(provider_main_loop(
                provider,
                client,
                oracle_addr.into(),
                move |instant, data| sender.send((monotonic_id, instant, data)).map_err(|_| ()),
                provider_name,
                tick_time,
                recovery_mode.clone(),
            ));
        });

    Ok((set, receiver))
}

async fn provider_main_loop<SenderFn>(
    provider: Box<dyn Provider + Send>,
    client: Arc<Client>,
    oracle_addr: String,
    sender: SenderFn,
    provider_name: String,
    tick_time: Duration,
    mut recovery_mode: watch::Receiver<bool>,
) -> Result<(), error::Worker>
where
    SenderFn: Fn(Instant, String) -> Result<(), ()>,
{
    let provider = { provider };

    let mut interval = interval(tick_time);

    let mut seq_error_counter = 0_u8;

    'worker_loop: loop {
        if select! {
            _ = interval.tick() => false,
            Ok(()) = recovery_mode.changed() => {
                *recovery_mode.borrow()
            }
        } {
            while *recovery_mode.borrow() {
                if recovery_mode.changed().await.is_err() {
                    error!("Recovery mode state watch closed! Exiting worker loop...");

                    break 'worker_loop Err(error::Worker::RecoveryModeWatchClosed);
                }
            }
        }

        let spot_prices_future = provider.get_spot_prices(&client, &oracle_addr);

        match spot_prices_future.await {
            Ok(prices) => {
                seq_error_counter = 0;

                let price_feed_json =
                    serde_json_wasm::to_string(&ExecuteMsg::FeedPrices { prices })?;

                if sender(Instant::now(), price_feed_json).is_err() {
                    info!(
                        provider_name = %provider_name,
                        "Communication channel has been closed! Exiting worker task..."
                    );

                    break 'worker_loop Ok(());
                }
            }
            Err(error) => {
                error!(
                    provider_name = %provider_name,
                    "Couldn't get price feed! Cause: {:?}",
                    error
                );

                if seq_error_counter == MAX_SEQ_ERRORS {
                    info!(provider_name = %provider_name, "Falling asleep...");

                    sleep(MAX_SEQ_ERRORS_SLEEP_DURATION).await;
                } else {
                    seq_error_counter += 1;
                }
            }
        };
    }
}
