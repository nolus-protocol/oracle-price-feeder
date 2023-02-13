use std::time::Duration;

use tokio::time::sleep;
use tracing::{debug, error, info, info_span};

use chain_comms::{
    build_tx::ContractTx,
    client::Client,
    config::Node,
    interact::{commit_tx_with_gas_estimation, query_wasm},
    log::{self, log_commit_response, setup_logging},
    rpc_setup::{prepare_rpc, RpcSetup},
    signer::Signer,
};
use semver::SemVer;

use self::{
    config::{Config, Contract},
    error::AppResult,
    messages::{DispatchResponse, ExecuteMsg, QueryMsg, StatusResponse},
};

pub mod config;
pub mod error;
pub mod messages;

pub const COMPATIBLE_VERSION: SemVer = SemVer::new(0, 2, 1);

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[tokio::main]
async fn main() -> AppResult<()> {
    let log_writer = tracing_appender::rolling::hourly("./dispatcher-logs", "log");

    let (log_writer, _guard) =
        tracing_appender::non_blocking(log::CombinedWriter::new(std::io::stdout(), log_writer));

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
    let rpc_setup =
        prepare_rpc::<Config, _>("alarms-dispatcher.toml", DEFAULT_COSMOS_HD_PATH).await?;

    info!("Checking compatibility with contract version...");

    for (contract, name) in [(rpc_setup.config.time_alarms(), "timealarms"), (rpc_setup.config.market_price_oracle(), "oracle")] {
        let version: SemVer = query_wasm(
            &rpc_setup.client,
            contract.address(),
            &serde_json_wasm::to_vec(&QueryMsg::ContractVersion {})?,
        )
        .await?;

        if !version.check_compatibility(COMPATIBLE_VERSION) {
            error!(
                compatible_minimum = %COMPATIBLE_VERSION,
                actual = %version,
                r#"Dispatcher version is incompatible with "{name}" contract's version!"#,
            );

            return Err(error::Application::IncompatibleContractVersion {
                contract: name,
                minimum_compatible: COMPATIBLE_VERSION,
                actual: version,
            });
        }
    }

    info!("Contract is compatible with feeder version.");

    let result = dispatch_alarms(rpc_setup).await;

    if let Err(error) = &result {
        error!("{error}");
    }

    info!("Shutting down...");

    result.map_err(Into::into)
}

async fn dispatch_alarms(
    RpcSetup {
        mut signer,
        config,
        client,
        ..
    }: RpcSetup<Config>,
) -> Result<(), error::DispatchAlarms> {
    let poll_period = Duration::from_secs(config.poll_period_seconds());

    let query = serde_json_wasm::to_vec(&QueryMsg::AlarmsStatus {})?;

    let mut fallback_gas_limit: u64 = 0;

    loop {
        for (contract, type_name, to_error) in [
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
        ] {
            fallback_gas_limit = dispatch_alarm(
                &mut signer,
                &client,
                config.node(),
                contract,
                &query,
                type_name,
                fallback_gas_limit,
            )
            .await
            .map_err(to_error)?
            .0
            .max(fallback_gas_limit);
        }

        sleep(poll_period).await;
    }
}

async fn dispatch_alarm<'r>(
    signer: &'r mut Signer,
    client: &'r Client,
    config: &'r Node,
    contract: &'r Contract,
    query: &'r [u8],
    alarm_type: &'static str,
    fallback_gas_limit: u64,
) -> Result<GasUsed, error::DispatchAlarm> {
    let mut max_gas_used: Option<GasUsed> = None;

    loop {
        let response: StatusResponse = query_wasm(client, contract.address(), query).await?;

        return Ok(if response.remaining_for_dispatch() {
            let result: CommitResult =
                commit_dispatch_tx(signer, client, config, contract, fallback_gas_limit).await?;

            info!(
                "Dispatched {} {} alarms.",
                result.dispatch_response.dispatched_alarms(),
                alarm_type
            );

            let max_gas_used: &mut GasUsed = &mut max_gas_used.get_or_insert(result.gas_used);

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
    fallback_gas_limit: u64,
) -> Result<CommitResult, error::CommitDispatchTx> {
    let unsigned_tx = ContractTx::new(contract.address().into()).add_message(
        serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
            max_count: contract.max_alarms_group(),
        })?,
        Vec::new(),
    );

    let tx_commit_response = commit_tx_with_gas_estimation(
        signer,
        client,
        config,
        contract
            .gas_limit_per_alarm()
            .saturating_mul(contract.max_alarms_group().into()),
        unsigned_tx,
        fallback_gas_limit,
    )
    .await?;

    let response =
        serde_json_wasm::from_slice::<DispatchResponse>(&tx_commit_response.deliver_tx.data);

    info_span!("Tx").in_scope(|| {
        if let Ok(response) = response.as_ref() {
            info!(
                "Dispatched {} alarms in total.",
                response.dispatched_alarms()
            );
        } else {
            error!("Failed to deserialize response data!");
            debug!(data = %String::from_utf8_lossy(&tx_commit_response.deliver_tx.data));
        }

        log_commit_response(&tx_commit_response);
    });

    Ok(CommitResult {
        dispatch_response: response?,
        gas_used: GasUsed(tx_commit_response.deliver_tx.gas_used.unsigned_abs()),
    })
}

pub struct CommitResult {
    dispatch_response: DispatchResponse,
    gas_used: GasUsed,
}
