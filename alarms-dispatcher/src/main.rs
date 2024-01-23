use std::{collections::BTreeMap, io, num::NonZeroU64, time::Duration};

use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};
use serde::Deserialize;
use tokio::{
    select, spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::{sleep, timeout, Instant},
};
use tracing::{error, error_span, info, warn};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use chain_comms::{
    client::Client as NodeClient,
    config::Node as NodeConfig,
    interact::{
        adjust_gas_limit, calculate_fee, commit, get_tx_response, process_simulation_result, query,
        simulate,
    },
    reexport::{
        cosmrs::{
            rpc::error::{Error as RpcError, ErrorDetail as RpcErrorDetail},
            tendermint::{abci::response::DeliverTx, Hash as TxHash},
            tx::Body as TxBody,
            Any as ProtobufAny,
        },
        tonic::transport::Channel as TonicChannel,
    },
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
    signing_key::DEFAULT_COSMOS_HD_PATH,
};

use self::{
    config::{Config, Contract},
    error::AppResult,
    messages::QueryMsg,
};

mod config;
mod error;
mod generators;
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

    check_compatibility(&rpc_setup).await?;

    info!("Contract is compatible with feeder version.");

    let result = dispatch_alarms(rpc_setup).await;

    if let Err(error) = &result {
        error!("{error}");
    }

    info!("Shutting down...");

    result.map_err(Into::into)
}

async fn check_compatibility(rpc_setup: &RpcSetup<Config>) -> AppResult<()> {
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
                query::wasm(rpc, contract.address.clone(), QueryMsg::CONTRACT_VERSION)
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
    let (tx_sender, mut tx_receiver) = unbounded_channel();

    let (mut tx_generators_set, mut result_senders) = generators::spawn(
        nolus_node,
        signer,
        &{ tx_sender },
        config.market_price_oracle,
        config.time_alarms,
        config.tick_time,
        config.poll_time,
    )?;

    let mut last_signing_timestamp = Instant::now();

    let mut fallback_gas_limit: u64 = 0;

    let mut process_transaction_env = ProcessTransactionRequestContext {
        node_client: nolus_node,
        node_config: &config.node,
        signer,
        tick_time: config.tick_time,
        poll_time: config.poll_time,
    };

    loop {
        select! {
            Some(_) = tx_generators_set.join_next(), if !tx_generators_set.is_empty() => {},
            Some((monotonic_id, messages, hard_gas_limit)) = tx_receiver.recv() => {
                if last_signing_timestamp.elapsed() < config.between_tx_margin_time {
                    sleep(config.between_tx_margin_time).await;
                }

                if let Some(ProcessTransactionRequestResult {
                    broadcast_timestamp,
                    new_fallback_gas_limit,
                    channel_closed,
                }) = process_transaction_request(
                    &mut process_transaction_env,
                    fallback_gas_limit,
                    &result_senders,
                    monotonic_id,
                    messages,
                    hard_gas_limit,
                )
                .await
                {
                    last_signing_timestamp = broadcast_timestamp;

                    fallback_gas_limit = new_fallback_gas_limit;

                    if channel_closed {
                        let _ = result_senders.remove(&monotonic_id);
                    }
                }
            }
            else => {
                info!("All generator threads stopped. Exiting.");

                break;
            }
        }
    }

    tx_generators_set.shutdown().await;

    Ok(())
}

struct ProcessTransactionRequestContext<'r> {
    node_client: &'r NodeClient,
    node_config: &'r NodeConfig,
    signer: &'r mut Signer,
    tick_time: Duration,
    poll_time: Duration,
}

struct ProcessTransactionRequestResult {
    broadcast_timestamp: Instant,
    new_fallback_gas_limit: u64,
    channel_closed: bool,
}

async fn process_transaction_request(
    &mut ProcessTransactionRequestContext {
        node_client,
        node_config,
        ref mut signer,
        tick_time,
        poll_time,
    }: &mut ProcessTransactionRequestContext<'_>,
    mut fallback_gas_limit: u64,
    result_senders: &BTreeMap<usize, CommitResultSender>,
    monotonic_id: usize,
    messages: Vec<ProtobufAny>,
    hard_gas_limit: NonZeroU64,
) -> Option<ProcessTransactionRequestResult> {
    let tx_body: TxBody = TxBody::new(messages, String::new(), 0_u32);

    let Some(signed_tx_bytes) =
        sign_and_serialize_tx(signer, node_config, hard_gas_limit.get(), tx_body.clone())
    else {
        return None;
    };

    let simulation_result =
        simulate::with_signed_body(node_client, signed_tx_bytes, hard_gas_limit.get()).await;

    let estimated_gas_limit = process_simulation_result(simulation_result, fallback_gas_limit);

    let gas_limit = adjust_gas_limit(node_config, estimated_gas_limit, hard_gas_limit.get());

    fallback_gas_limit = gas_limit.max(fallback_gas_limit);

    let Some(signed_tx_bytes) = sign_and_serialize_tx(signer, node_config, gas_limit, tx_body)
    else {
        return None;
    };

    let tx_response = broadcast_commit(node_client, signer, signed_tx_bytes).await;

    let broadcast_timestamp = Instant::now();

    log::commit_response(&tx_response);

    let channel_closed = matches!(
        send_back_tx_hash(
            node_client,
            tick_time,
            poll_time,
            result_senders,
            monotonic_id,
            tx_response,
        ),
        SendBackTxHashResult::ChannelClosed
    );

    Some(ProcessTransactionRequestResult {
        broadcast_timestamp,
        new_fallback_gas_limit: fallback_gas_limit,
        channel_closed,
    })
}

enum SendBackTxHashResult {
    Ok,
    ChannelClosed,
}

fn send_back_tx_hash(
    node_client: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    result_senders: &BTreeMap<usize, CommitResultSender>,
    monotonic_id: usize,
    tx_response: commit::Response,
) -> SendBackTxHashResult {
    let hash = tx_response.hash;

    let channel_closed = if let Some(sender) = result_senders.get(&monotonic_id) {
        if sender
            .send(if tx_response.code.is_ok() {
                Ok(tx_response.hash)
            } else {
                Err(CommitError {
                    r#type: if tx_response.code.value() == 32 {
                        CommitErrorType::InvalidAccountSequence
                    } else {
                        CommitErrorType::Unknown
                    },
                    response: tx_response,
                })
            })
            .is_ok()
        {
            return SendBackTxHashResult::Ok;
        }

        SendBackTxHashResult::ChannelClosed
    } else {
        SendBackTxHashResult::Ok
    };

    let node_client = node_client.clone();

    spawn(async move {
        poll_delivered_tx(&node_client, tick_time, poll_time, hash).await;
    });

    channel_closed
}

async fn broadcast_commit(
    node_client: &NodeClient,
    signer: &mut Signer,
    signed_tx_bytes: Vec<u8>,
) -> commit::Response {
    loop {
        match commit::with_signed_body(node_client, signed_tx_bytes.clone(), signer).await {
            Ok(tx_response) => break tx_response,
            Err(error) => {
                error_span!("Broadcast").in_scope(|| {
                    if let commit::error::CommitTx::Broadcast(
                        RpcError(
                            RpcErrorDetail::Timeout(..),
                            ..,
                        ),
                    ) = error {
                        warn!(error = ?error, "Failed to broadcast transaction due to a timeout! Cause: {}", error);
                    } else {
                        error!(error = ?error, "Failed to broadcast transaction due to an error! Cause: {}", error);
                    }

                    info!("Retrying to broadcast.");
                });
            }
        }
    }
}

fn sign_and_serialize_tx(
    signer: &mut Signer,
    node_config: &NodeConfig,
    gas_limit: u64,
    tx_body: TxBody,
) -> Option<Vec<u8>> {
    match signer.sign(tx_body, calculate_fee(node_config, gas_limit)) {
        Ok(signed_tx) => match signed_tx.to_bytes() {
            Ok(tx_bytes) => {
                return Some(tx_bytes);
            }
            Err(error) => {
                error!(error = ?error, "Serializing signed transaction failed! Cause: {}", error);
            }
        },
        Err(error) => {
            error!(error = ?error, "Signing transaction failed! Cause: {}", error);
        }
    }

    None
}

enum CommitErrorType {
    InvalidAccountSequence,
    Unknown,
}

struct CommitError {
    r#type: CommitErrorType,
    response: commit::Response,
}

type CommitResult = Result<TxHash, CommitError>;

type CommitResultSender = UnboundedSender<CommitResult>;

type CommitResultReceiver = UnboundedReceiver<CommitResult>;

async fn poll_delivered_tx(
    nolus_node: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    hash: TxHash,
) -> Option<DeliverTx> {
    timeout(tick_time, async {
        loop {
            sleep(poll_time).await;

            match get_tx_response(nolus_node, hash).await {
                Ok(tx) => break tx,
                Err(error) => error!(
                    hash = %hash,
                    error = ?error,
                    "Polling delivered transaction failed!",
                ),
            }
        }
    })
    .await
    .ok()
}
