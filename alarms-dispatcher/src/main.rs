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

use chain_comms::{
    interact::query,
    reexport::tonic::transport::Channel as TonicChannel,
    rpc_setup::{prepare_rpc, RpcSetup},
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
            .node_client
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
        signer,
        config,
        node_client,
        ..
    }: RpcSetup<Config>,
) -> Result<(), error::DispatchAlarms> {
    let spawn_generators = {
        let node_client = node_client.clone();

        let signer_address = signer.signer_address().to_owned();

        let tick_time = config.broadcast.tick_time;

        let poll_time = config.broadcast.poll_time;

        move |tx_sender| {
            generators::spawn(
                &node_client,
                signer_address,
                &{ tx_sender },
                config.market_price_oracle,
                config.time_alarms,
                tick_time,
                poll_time,
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
