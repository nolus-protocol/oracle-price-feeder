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

use self::{
    config::Config,
    messages::{DispatchResponse, ExecuteMsg, QueryMsg, StatusResponse},
};

pub mod config;
pub mod error;
pub mod messages;

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_CONSEQUENT_ERRORS_COUNT: usize = 5;

#[tokio::main]
async fn main() -> Result<(), error::Application> {
    let log_writer = tracing_appender::rolling::hourly("./dispatcher-logs", "log");

    let (log_writer, _guard) =
        tracing_appender::non_blocking(log::CombinedWriter::new(std::io::stdout(), log_writer));

    setup_logging(log_writer)?;

    info!(concat!(
        "Running version built on: ",
        env!("BUILD_START_TIME_DATE", "No build time provided!")
    ));

    let result = dispatch_alarms(
        prepare_rpc::<Config, _>("alarms-dispatcher.toml", DEFAULT_COSMOS_HD_PATH).await?,
    )
    .await;

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
            dispatch_alarm(
                &mut signer,
                &client,
                config.node(),
                contract.address(),
                contract.gas_limit_per_alarm(),
                contract.max_alarms_group(),
                &query,
                type_name,
            )
            .await
            .map_err(to_error)?;
        }

        sleep(poll_period).await;
    }
}

async fn dispatch_alarm<'r>(
    signer: &'r mut Signer,
    client: &'r Client,
    config: &'r Node,
    address: &'r str,
    gas_limit_per_alarm: u64,
    max_alarms: u32,
    query: &'r [u8],
    alarm_type: &'static str,
) -> Result<(), error::DispatchAlarm> {
    loop {
        let response: StatusResponse = query_wasm(client, address, query).await?;

        if response.remaining_for_dispatch() {
            let result: DispatchResponse = commit_dispatch_tx(
                signer,
                client,
                config,
                address,
                gas_limit_per_alarm,
                max_alarms,
            )
            .await?;

            info!(
                "Dispatched {} {} alarms.",
                result.dispatched_alarms(),
                alarm_type
            );

            if result.dispatched_alarms() == max_alarms {
                continue;
            }
        } else {
            debug!("Queue for {} alarms is empty.", alarm_type);
        }

        return Ok(());
    }
}

async fn commit_dispatch_tx(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    address: &str,
    gas_limit_per_alarm: u64,
    max_count: u32,
) -> Result<DispatchResponse, error::CommitDispatchTx> {
    let unsigned_tx = ContractTx::new(address.into()).add_message(
        serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms { max_count })?,
        Vec::new(),
    );

    let tx_commit_response = commit_tx_with_gas_estimation(
        signer,
        client,
        config,
        gas_limit_per_alarm.saturating_mul(max_count.into()),
        unsigned_tx,
    )
    .await?;

    let response =
        serde_json_wasm::from_slice::<DispatchResponse>(&tx_commit_response.deliver_tx.data);

    info_span!("Tx").in_scope(|| {
        if let Ok(response) = &response {
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

    response.map_err(Into::into)
}
