use std::{io, time::Duration};

use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};
use serde::Deserialize;
use tokio::time::sleep;
use tracing::{debug, error, info, info_span, span::EnteredSpan};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use chain_comms::{
    build_tx::ContractTx,
    client::Client,
    config::Node,
    decode,
    interact::{commit_tx_with_gas_estimation, query_wasm, CommitResponse},
    log::{self, commit_response},
    reexport::tonic::transport::Channel as TonicChannel,
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
    signing_key::DEFAULT_COSMOS_HD_PATH,
};

use self::{
    config::{Config, Contract},
    error::AppResult,
    messages::{DispatchResponse, ExecuteMsg, QueryMsg, StatusResponse},
};

pub mod config;
pub mod error;
pub mod messages;

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

    log::setup(io::stdout.and(log_writer));

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

    for (contract, name, compatible) in [
        (
            rpc_setup.config.time_alarms(),
            "timealarms",
            TIME_ALARMS_COMPATIBLE_VERSION,
        ),
        (
            rpc_setup.config.market_price_oracle(),
            "oracle",
            ORACLE_COMPATIBLE_VERSION,
        ),
    ] {
        let version: JsonVersion = rpc_setup
            .nolus_node
            .with_grpc(|rpc: TonicChannel| {
                query_wasm(
                    rpc,
                    contract.address().to_string(),
                    QueryMsg::CONTRACT_VERSION,
                )
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
        ref config,
        nolus_node: ref client,
        ..
    }: RpcSetup<Config>,
) -> Result<(), error::DispatchAlarms> {
    type Contracts<'r> = [(
        &'r Contract,
        &'static str,
        fn(error::DispatchAlarm) -> error::DispatchAlarms,
    ); 2];

    let poll_period: Duration = Duration::from_secs(config.poll_period_seconds());

    let mut fallback_gas_limit: Option<u64> = None;

    let contracts: Contracts<'_> = [
        (
            config.market_price_oracle(),
            "market price",
            error::DispatchAlarms::DispatchPriceAlarm
                as fn(error::DispatchAlarm) -> error::DispatchAlarms,
        ),
        (
            config.time_alarms(),
            "time",
            error::DispatchAlarms::DispatchTimeAlarm
                as fn(error::DispatchAlarm) -> error::DispatchAlarms,
        ),
    ];

    loop {
        for contract in contracts {
            let mut is_retry: bool = false;

            'dispatch_alarm: loop {
                match handle_alarms_dispatch(
                    signer,
                    config,
                    client,
                    contract,
                    QueryMsg::ALARMS_STATUS,
                    &mut fallback_gas_limit,
                )
                .await
                {
                    DispatchResult::Success => break 'dispatch_alarm,
                    DispatchResult::DispatchFailed => {
                        if is_retry {
                            break 'dispatch_alarm;
                        }

                        is_retry = true;
                    }
                }
            }
        }

        sleep(poll_period).await;
    }
}

enum DispatchResult {
    Success,
    DispatchFailed,
}

async fn handle_alarms_dispatch<'r>(
    signer: &mut Signer,
    config: &Config,
    client: &Client,
    (contract, alarm_type_name, to_error): (
        &'r Contract,
        &'static str,
        fn(error::DispatchAlarm) -> error::DispatchAlarms,
    ),
    query: &'r [u8],
    fallback_gas_limit: &mut Option<u64>,
) -> DispatchResult {
    let result: Result<GasUsed, error::DispatchAlarms> = dispatch_alarm(
        signer,
        client,
        config.node(),
        contract,
        query,
        alarm_type_name,
        *fallback_gas_limit,
    )
    .await
    .map_err(to_error);

    match result {
        Ok(gas_used) => {
            let fallback_gas_limit: &mut u64 = fallback_gas_limit.get_or_insert(gas_used.0);

            *fallback_gas_limit = gas_used.0.max(*fallback_gas_limit);

            DispatchResult::Success
        }
        Err(error) => {
            let span: EnteredSpan = info_span!("dispatch-error").entered();

            error!("{error}");

            if signer.needs_update() {
                drop(span);

                let recovered: bool = recover_after_error(signer, client).await;

                if !recovered {
                    while !recover_after_error(signer, client).await {
                        sleep(Duration::from_secs(15)).await;
                    }
                }
            }

            DispatchResult::DispatchFailed
        }
    }
}

async fn recover_after_error(signer: &mut Signer, client: &Client) -> bool {
    if let Err(error) = signer.update_account(client).await {
        error!("{error}");

        return false;
    }

    info!("Successfully updated local copy of account data.");

    info!("Continuing normal workflow...");

    true
}

async fn dispatch_alarm<'r>(
    signer: &'r mut Signer,
    client: &'r Client,
    config: &'r Node,
    contract: &'r Contract,
    query: &'r [u8],
    alarm_type: &'static str,
    fallback_gas_limit: Option<u64>,
) -> Result<GasUsed, error::DispatchAlarm> {
    let mut max_gas_used: Option<GasUsed> = None;

    loop {
        let response: StatusResponse = client
            .with_grpc(|rpc: TonicChannel| query_wasm(rpc, contract.address().to_string(), query))
            .await?;

        return Ok(if response.remaining_for_dispatch() {
            let result: CommitResult =
                commit_dispatch_tx(signer, client, config, contract, fallback_gas_limit).await?;

            info!(
                "Dispatched {} {} alarms.",
                result.dispatch_response.dispatched_alarms(),
                alarm_type
            );

            let max_gas_used: &mut GasUsed = max_gas_used.get_or_insert(result.gas_used);

            *max_gas_used = Ord::max(*max_gas_used, result.gas_used);

            if result.dispatch_response.dispatched_alarms() == contract.max_alarms_group() {
                continue;
            }

            return Ok(*max_gas_used);
        } else {
            debug!("Queue for {} alarms is empty.", alarm_type);

            max_gas_used.unwrap_or_default()
        });
    }
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Default)]
struct GasUsed(u64);

async fn commit_dispatch_tx(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    contract: &Contract,
    fallback_gas_limit: Option<u64>,
) -> Result<CommitResult, error::CommitDispatchTx> {
    let unsigned_tx: ContractTx = ContractTx::new(contract.address().into()).add_message(
        serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
            max_count: contract.max_alarms_group(),
        })?,
        Vec::new(),
    );

    let gas_limit: u64 = contract
        .gas_limit_per_alarm()
        .saturating_mul(contract.max_alarms_group().into());

    let tx_commit_response: CommitResponse = commit_tx_with_gas_estimation(
        signer,
        client,
        config,
        gas_limit,
        unsigned_tx,
        fallback_gas_limit.unwrap_or(gas_limit),
    )
    .await?;

    let raw_response: Vec<u8> = decode::exec_tx_data(&tx_commit_response.deliver_tx)?;

    let response: Result<DispatchResponse, serde_json_wasm::de::Error> =
        serde_json_wasm::from_slice::<DispatchResponse>(&raw_response);

    let response: Result<DispatchResponse, error::CommitDispatchTx> =
        response.map_err(|error: serde_json_wasm::de::Error| {
            error::CommitDispatchTx::DeserializeDispatchResponse(
                error,
                String::from_utf8_lossy(&raw_response).into(),
            )
        });

    let successful: bool =
        tx_commit_response.check_tx.code.is_ok() && tx_commit_response.deliver_tx.code.is_ok();

    info_span!("Tx").in_scope(|| {
        if successful {
            if let Ok(response) = response.as_ref() {
                info!(
                    "Dispatched {} alarms in total.",
                    response.dispatched_alarms()
                );
            } else {
                error!("Transaction result couldn't be interpreted!");
            }
        }

        commit_response(&tx_commit_response);
    });

    if successful {
        response.map(|dispatch_response: DispatchResponse| CommitResult {
            dispatch_response,
            gas_used: GasUsed(tx_commit_response.deliver_tx.gas_used.unsigned_abs()),
        })
    } else {
        Err(error::CommitDispatchTx::TxFailed(
            if tx_commit_response.check_tx.code.is_err() {
                tx_commit_response.check_tx.log
            } else {
                tx_commit_response.deliver_tx.log
            },
        ))
    }
}

pub struct CommitResult {
    dispatch_response: DispatchResponse,
    gas_used: GasUsed,
}
