#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_errors_doc,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening
)]

use std::io;

use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};
use serde::Deserialize;
use tracing::{error, info};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use crate::generators::TasksConfig;
use chain_comms::{
    client::Client as NodeClient,
    interact::query,
    rpc_setup::{prepare_rpc, RpcSetup},
    signing_key::DEFAULT_COSMOS_HD_PATH,
};

use self::{config::Config, error::AppResult, generators::Contract, messages::QueryMsg};

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

#[allow(clippy::future_not_send)]
async fn app_main() -> AppResult<()> {
    let rpc_setup: RpcSetup<Config> =
        prepare_rpc::<Config, _>("alarms-dispatcher.toml", DEFAULT_COSMOS_HD_PATH).await?;

    info!("Fetching all relevant contracts...");

    let contracts = fetch_contracts(&rpc_setup.node_client, &rpc_setup.config).await?;

    info!("Checking compatibility with contract version...");

    check_compatibility(&rpc_setup, &contracts).await?;

    info!("Contract is compatible with feeder version.");

    let result = dispatch_alarms(rpc_setup, contracts.into_iter()).await;

    if let Err(error) = &result {
        error!("{error}");
    }

    info!("Shutting down...");

    result.map_err(Into::into)
}

async fn fetch_contracts(node_client: &NodeClient, config: &Config) -> AppResult<Vec<Contract>> {
    let platform_contracts =
        platform::Platform::fetch(node_client, config.admin_contract.clone().into_string()).await?;

    let protocols =
        platform::Protocols::fetch(node_client, config.admin_contract.clone().into_string())
            .await?;

    let mut contracts = Vec::with_capacity(protocols.0.len() + 1);

    contracts.push(Contract::TimeAlarms(platform_contracts.time_alarms));

    for protocol in protocols.0.into_vec() {
        contracts.push(Contract::Oracle(
            protocol
                .fetch(node_client, config.admin_contract.clone().into_string())
                .await?
                .contracts
                .oracle,
        ));
    }

    Ok(contracts)
}

#[allow(clippy::future_not_send)]
async fn check_compatibility(
    rpc_setup: &RpcSetup<Config>,
    contracts: &[Contract],
) -> AppResult<()> {
    #[derive(Deserialize)]
    struct JsonVersion {
        major: u64,
        minor: u64,
        patch: u64,
    }

    let contracts_iter = contracts.iter().map(|contract| match contract {
        Contract::TimeAlarms(contract) => (contract, "time_alarms", TIME_ALARMS_COMPATIBLE_VERSION),
        Contract::Oracle(contract) => (contract, "oracle", ORACLE_COMPATIBLE_VERSION),
    });

    for (contract, name, compatible) in contracts_iter {
        let version: JsonVersion = query::wasm_smart(
            &mut rpc_setup.node_client.wasm_query_client(),
            contract.clone().into_string(),
            QueryMsg::CONTRACT_VERSION.to_vec(),
        )
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

#[allow(clippy::future_not_send)]
async fn dispatch_alarms<I>(
    RpcSetup {
        signer,
        config,
        node_client,
        ..
    }: RpcSetup<Config>,
    contracts: I,
) -> Result<(), error::DispatchAlarms>
where
    I: Iterator<Item = Contract> + Send,
{
    let spawn_generators = {
        let node_client = node_client.clone();

        let signer_address = signer.signer_address().to_owned();

        let tick_time = config.broadcast.tick_time;

        let poll_time = config.broadcast.poll_time;

        move |tx_sender| {
            generators::spawn(
                &node_client,
                &{ signer_address },
                &{ tx_sender },
                &TasksConfig {
                    time_alarms_config: config.time_alarms,
                    oracle_alarms_config: config.market_price_oracle,
                    tick_time,
                    poll_time,
                },
                contracts,
            )
        }
    };

    broadcast::broadcast(
        signer,
        config.broadcast,
        node_client,
        config.node,
        spawn_generators,
    )
    .await
}
