use std::{collections::BTreeMap, io, str::FromStr, sync::Arc, time::Duration};

use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver},
    task::JoinSet,
    time::{interval, sleep, timeout, Instant},
};
use tracing::{error, info};

use chain_comms::{
    build_tx::ContractTx,
    client::Client,
    interact::{commit_tx_with_gas_estimation, query_wasm, CommitResponse},
    log::{self, log_commit_response, setup_logging},
    rpc_setup::{prepare_rpc, RpcSetup},
};
use semver::SemVer;

use self::{
    config::{Config, Providers},
    error::AppResult,
    messages::{ExecuteMsg, QueryMsg},
    provider::{Factory, Provider, Type},
};

pub mod config;
pub mod error;
pub mod messages;
pub mod provider;

pub mod tests;

pub const COMPATIBLE_VERSION: SemVer = SemVer::new(0, 2, 1);

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

    info!("Checking compatibility with contract version...");

    {
        let version: SemVer = query_wasm(
            &client,
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

    let client = Arc::new(client);

    info!("Starting workers...");

    let tick_time = Duration::from_secs(config.tick_time());

    let (mut set, mut receiver) =
        spawn_workers(&client, config.providers(), config.oracle_addr(), tick_time)?;

    info!("Workers started. Entering broadcasting loop...");

    let mut fallback_gas_limit: u64 = 0;

    loop {
        let mut messages: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

        let channel_closed = timeout(tick_time, async {
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
            continue;
        }

        let tx = messages.into_values().fold(
            ContractTx::new(config.oracle_addr().to_string()),
            |tx, msg| tx.add_message(msg, Vec::new()),
        );

        let response: CommitResponse = commit_tx_with_gas_estimation(
            &mut signer,
            &client,
            config.as_ref(),
            config.gas_limit(),
            tx,
            fallback_gas_limit,
        )
        .await?;

        fallback_gas_limit = response
            .deliver_tx
            .gas_used
            .unsigned_abs()
            .max(fallback_gas_limit);

        log_commit_response(&response);

        if channel_closed {
            drop(receiver);

            break;
        }
    }

    while set.join_next().await.is_some() {}

    Ok(())
}

type SpawnWorkersResult = AppResult<(
    JoinSet<Result<(), error::Worker>>,
    UnboundedReceiver<(usize, Instant, String)>,
)>;

fn spawn_workers(
    client: &Arc<Client>,
    providers: &[Providers],
    oracle_addr: &str,
    tick_time: Duration,
) -> SpawnWorkersResult {
    let mut set = JoinSet::new();

    let (sender, receiver) = unbounded_channel();

    providers
        .into_iter()
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
) -> Result<(), error::Worker>
where
    SenderFn: Fn(Instant, String) -> Result<(), ()>,
{
    let provider = { provider };

    let mut interval = interval(tick_time);

    let mut seq_error_counter = 0_u8;

    loop {
        interval.tick().await;

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

                    return Ok(());
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
