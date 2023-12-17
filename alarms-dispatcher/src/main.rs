use std::{io, num::NonZeroU64, sync::Arc, time::Duration};

use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};
use serde::Deserialize;
use tokio::{
    select,
    sync::mpsc::{unbounded_channel, UnboundedSender},
    task::JoinSet,
    time::{sleep as tokio_sleep, sleep_until as tokio_sleep_until, Instant},
};
use tracing::{error, error_span, info, warn};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use chain_comms::{
    interact::{commit_tx_with_gas_estimation_and_serialized_message, get_tx_response, query_wasm},
    reexport::{
        cosmrs::{
            proto::cosmwasm::wasm::v1::MsgExecuteContract, tx::MessageExt as _, Any as ProtobufAny,
        },
        tonic::transport::Channel as TonicChannel,
    },
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
    signing_key::DEFAULT_COSMOS_HD_PATH,
};

use crate::messages::StatusResponse;

use self::{
    config::{Config, Contract},
    error::AppResult,
    messages::{ExecuteMsg, QueryMsg},
};

mod config;
mod error;
mod log;
mod messages;

pub const ORACLE_COMPATIBLE_VERSION: SemVerComparator = SemVerComparator {
    op: semver::Op::GreaterEq,
    major: 0,
    minor: Some(5),
    patch: None,
    pre: SemVerPrerelease::EMPTY,
};
pub const TIME_ALARMS_COMPATIBLE_VERSION: SemVerComparator = SemVerComparator {
    op: semver::Op::GreaterEq,
    major: 0,
    minor: Some(4),
    patch: Some(1),
    pre: SemVerPrerelease::EMPTY,
};

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[tokio::main]
async fn main() -> AppResult<()> {
    let (log_writer, log_guard): (NonBlocking, non_blocking::WorkerGuard) =
        NonBlocking::new(rolling::hourly("./dispatcher-logs", "log"));

    chain_comms::log::setup(io::stdout.and(log_writer));

    info!(concat!(
        "Running version built on: ",
        env!("BUILD_START_TIME_DATE", "No build time provided!")
    ));

    let result: AppResult<()> = app_main().await;

    if let Err(error) = &result {
        error!(error = ?error, "{}", error);
    }

    drop(log_guard);

    result
}

async fn app_main() -> AppResult<()> {
    let rpc_setup: RpcSetup<Config> =
        prepare_rpc::<Config, _>("alarms-dispatcher.toml", DEFAULT_COSMOS_HD_PATH).await?;

    info!("Checking compatibility with contract version...");

    check_comparibility(&rpc_setup).await?;

    info!("Contract is compatible with feeder version.");

    let result = dispatch_alarms(rpc_setup).await;

    if let Err(error) = &result {
        error!("{error}");
    }

    info!("Shutting down...");

    result.map_err(Into::into)
}

async fn check_comparibility(rpc_setup: &RpcSetup<Config>) -> AppResult<()> {
    #[derive(Deserialize)]
    struct JsonVersion {
        major: u64,
        minor: u64,
        patch: u64,
    }

    for (contract, name, compatible) in rpc_setup
        .config
        .time_alarms
        .iter()
        .map(|contract: &Contract| (contract, "timealarms", TIME_ALARMS_COMPATIBLE_VERSION))
        .chain(
            rpc_setup
                .config
                .market_price_oracle
                .iter()
                .map(|contract: &Contract| (contract, "oracle", ORACLE_COMPATIBLE_VERSION)),
        )
    {
        let version: JsonVersion = rpc_setup
            .nolus_node
            .with_grpc(|rpc: TonicChannel| {
                query_wasm(rpc, contract.address.clone(), QueryMsg::CONTRACT_VERSION)
            })
            .await?;

        let version: Version = Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            pre: SemVerPrerelease::EMPTY,
            build: SemVerBuildMetadata::EMPTY,
        };

        if !compatible.matches(&version) {
            error!(
                compatible = %compatible,
                actual = %version,
                r#"Dispatcher version is incompatible with "{name}" contract's version!"#,
            );

            return Err(error::Application::IncompatibleContractVersion {
                contract: name,
                compatible,
                actual: version,
            });
        }
    }

    Ok(())
}

async fn dispatch_alarms(
    RpcSetup {
        ref mut signer,
        config,
        ref nolus_node,
        ..
    }: RpcSetup<Config>,
) -> Result<(), error::DispatchAlarms> {
    let mut fallback_gas_limit: Option<NonZeroU64> = None;

    let mut periodic_tx_generators_set = JoinSet::new();
    let mut delivered_tx_fetchers_set = JoinSet::new();

    let (tx_sender, mut tx_receiver) = unbounded_channel();

    spawn_periodic_tx_generators(
        signer,
        config.poll_period,
        config.market_price_oracle,
        config.time_alarms,
        tx_sender.clone(),
        &mut periodic_tx_generators_set,
        nolus_node,
    )?;

    let tx_sender = {
        let downgraded = tx_sender.downgrade();

        drop(tx_sender);

        downgraded
    };

    let mut next_signing_timestamp = Instant::now();

    loop {
        select! {
            Some(result) = delivered_tx_fetchers_set.join_next(), if !delivered_tx_fetchers_set.is_empty() => {
                match result {
                    Ok(Some(gas_used)) => {
                        fallback_gas_limit = Some(fallback_gas_limit.unwrap_or(gas_used).max(gas_used))
                    }
                    Ok(None) => {}
                    Err(error) => error!(
                        error = ?error,
                        "Failure reported back from delivered transaction logger! Probable cause is a panic! Error: {error}"
                    ),
                }
            }
            Some((message, hard_gas_limit, tx_time, contract_type, contract_address)) = tx_receiver.recv() => {
                if next_signing_timestamp.elapsed() < config.between_tx_margin_time {
                    tokio_sleep_until(next_signing_timestamp).await;
                }

                let result = commit_tx_with_gas_estimation_and_serialized_message(
                    signer,
                    nolus_node,
                    &config.node,
                    hard_gas_limit.get(),
                    fallback_gas_limit.map_or(hard_gas_limit.get(), NonZeroU64::get),
                    message.clone(),
                )
                .await;

                next_signing_timestamp = Instant::now();

                match result {
                    Ok(response) => {
                        self::log::commit_response(contract_type, &contract_address, &response);

                        let hash = response.hash;
                        let client = nolus_node.clone();
                        let retry_sender = tx_sender.clone();

                        delivered_tx_fetchers_set.spawn(async move {
                            tokio_sleep(config.query_delivered_tx_tick_time).await;

                            match get_tx_response(&client, response.hash).await {
                                Ok(response) => {
                                    self::log::tx_response(contract_type, &contract_address, &hash, &response);

                                    NonZeroU64::new(response.gas_used.unsigned_abs())
                                        .map(|gas_limit| gas_limit.min(hard_gas_limit))
                                }
                                Err(error) => {
                                    error_span!(
                                        "Delivered Tx",
                                        contract_type = contract_type,
                                        contract_address = contract_address.as_ref(),
                                    )
                                    .in_scope(|| {
                                        error!(
                                            error = ?error,
                                            "Failed to fetch transaction response! Cause: {error}",
                                        );

                                        info!("Sending transaction for retry.");

                                        if tx_time.elapsed() >= config.poll_period {
                                            warn!("Transaction timed-out.");
                                        } else if retry_sender
                                            .upgrade()
                                            .and_then(|tx_sender| {
                                                tx_sender
                                                    .send((
                                                        message,
                                                        hard_gas_limit,
                                                        tx_time,
                                                        contract_type,
                                                        contract_address,
                                                    ))
                                                    .err()
                                            })
                                            .is_some()
                                        {
                                            warn!("Sending for retry failed. Channel is closed.");
                                        }
                                    });

                                    None
                                }
                            }
                        });
                    }
                    Err(error) => error!(
                        contract_type = contract_type,
                        contract_address = contract_address.as_ref(),
                        error = ?error,
                        "Failed to commit alarms dispatching transaction! Cause: {error}",
                    ),
                }
            }
            else => {
                warn!("Transaction receiving channel is closed!");

                break;
            }
        }
    }

    periodic_tx_generators_set.shutdown().await;
    delivered_tx_fetchers_set.shutdown().await;

    Ok(())
}

type TxGeneratorsSender = UnboundedSender<(
    Vec<ProtobufAny>,
    NonZeroU64,
    Instant,
    &'static str,
    Arc<str>,
)>;

fn spawn_periodic_tx_generators(
    signer: &Signer,
    poll_period: Duration,
    market_price_oracle_contracts: Vec<Contract>,
    time_alarms_contracts: Vec<Contract>,
    tx_sender: TxGeneratorsSender,
    periodic_tx_generators_set: &mut JoinSet<()>,
    nolus_node: &chain_comms::client::Client,
) -> Result<(), error::DispatchAlarms> {
    market_price_oracle_contracts
        .into_iter()
        .map(|contract| (contract, "oracle"))
        .chain(
            time_alarms_contracts
                .into_iter()
                .map(|contract| (contract, "time_alarms")),
        )
        .try_for_each(
            |(contract, contract_type)| -> Result<(), error::DispatchAlarms> {
                let message: Arc<[ProtobufAny]> = {
                    let mut message: Vec<ProtobufAny> = vec![MsgExecuteContract {
                        sender: signer.signer_address().into(),
                        contract: contract.address.clone(),
                        msg: serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
                            max_count: contract.max_alarms_group.get(),
                        })?,
                        funds: Vec::new(),
                    }
                    .to_any()?];

                    message.shrink_to_fit();

                    message.into()
                };

                let contract_address: Arc<str> = contract.address.into();

                let hard_gas_limit = contract
                    .gas_limit_per_alarm
                    .saturating_mul(contract.max_alarms_group.into());

                let tx_sender = tx_sender.clone();

                periodic_tx_generators_set.spawn({
                    let nolus_node = nolus_node.clone();

                    async move {
                        loop {
                            let should_send = nolus_node
                                .with_grpc({
                                    let contract_address: String = contract_address.to_string();

                                    |rpc| async move {
                                        query_wasm(rpc, contract_address, QueryMsg::ALARMS_STATUS)
                                            .await
                                    }
                                })
                                .await
                                .map_or(true, |response: StatusResponse| {
                                    response.remaining_for_dispatch()
                                });

                            if should_send {
                                let channel_closed = tx_sender
                                    .send((
                                        message.to_vec(),
                                        hard_gas_limit,
                                        Instant::now(),
                                        contract_type,
                                        contract_address.clone(),
                                    ))
                                    .is_err();

                                if channel_closed {
                                    warn!(
                                        contract_type = contract_type,
                                        contract_address = contract_address.as_ref(),
                                        "Channel closed. Exiting task.",
                                    );

                                    break;
                                }
                            }

                            tokio_sleep(poll_period).await;
                        }
                    }
                });

                Ok(())
            },
        )
}
