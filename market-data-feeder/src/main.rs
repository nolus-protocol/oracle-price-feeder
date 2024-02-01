#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::missing_errors_doc,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening
)]

use std::{io, sync::Arc};

use semver::{
    BuildMetadata as SemVerBuildMetadata, Comparator as SemVerComparator,
    Prerelease as SemVerPrerelease, Version,
};
use serde::Deserialize;
use tokio::task::block_in_place;
use tracing::{error, info};
use tracing_appender::{
    non_blocking::{self, NonBlocking},
    rolling,
};
use tracing_subscriber::fmt::writer::MakeWriterExt as _;

use broadcast::broadcast;
use chain_comms::{
    client::Client as NodeClient,
    interact::query,
    reexport::tonic::transport::Channel as TonicChannel,
    rpc_setup::{prepare_rpc, RpcSetup},
    signing_key::DEFAULT_COSMOS_HD_PATH,
};

use self::{config::Config, messages::QueryMsg, result::Result, workers::SpawnContext};

mod config;
mod deviation;
mod error;
mod log;
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

#[tokio::main]
async fn main() -> Result<()> {
    let (log_writer, log_guard): (NonBlocking, non_blocking::WorkerGuard) =
        NonBlocking::new(rolling::hourly("./feeder-logs", "log"));

    chain_comms::log::setup(io::stdout.and(log_writer));

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
        signer,
        config,
        node_client,
        ..
    }: RpcSetup<Config> = prepare_rpc("market-data-feeder.toml", DEFAULT_COSMOS_HD_PATH).await?;

    check_compatibility(&config, &node_client).await?;

    let spawn_generators_f = {
        let node_client: NodeClient = node_client.clone();

        let signer_address: Arc<str> = Arc::from(signer.signer_address());

        move |tx_request_sender| {
            info!("Starting workers...");

            block_in_place(move || {
                workers::spawn(SpawnContext {
                    node_client: node_client.clone(),
                    providers: config.providers,
                    price_comparison_providers: config.comparison_providers,
                    tx_request_sender,
                    signer_address,
                    hard_gas_limit: config.hard_gas_limit,
                    tick_time: config.broadcast.tick_time,
                    poll_time: config.broadcast.poll_time,
                })
            })
            .map(|spawn_result| {
                info!("Workers started successfully.");

                spawn_result
            })
        }
    };

    broadcast::<broadcast::mode::NonBlocking, _, _, _>(
        signer,
        config.broadcast,
        node_client,
        config.node,
        spawn_generators_f,
    )
    .await
}

async fn check_compatibility(config: &Config, node_client: &NodeClient) -> Result<()> {
    #[derive(Deserialize)]
    struct JsonVersion {
        major: u64,
        minor: u64,
        patch: u64,
    }

    info!("Checking compatibility with contract version...");

    for (oracle_name, oracle_address) in &config.oracles {
        let version: JsonVersion = node_client
            .with_grpc(|rpc: TonicChannel| {
                query::wasm(rpc, oracle_address.to_string(), QueryMsg::CONTRACT_VERSION)
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
                oracle = %oracle_name,
                compatible = %COMPATIBLE_VERSION,
                actual = %version,
                "Feeder version is incompatible with contract version!"
            );

            return Err(error::Application::IncompatibleContractVersion {
                oracle_addr: oracle_address.clone(),
                compatible: COMPATIBLE_VERSION,
                actual: version,
            });
        }
    }

    info!("Contract is compatible with feeder version.");

    Ok(())
}
