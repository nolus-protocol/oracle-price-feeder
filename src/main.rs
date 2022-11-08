use std::{io, process::exit, str::FromStr, sync::Arc, time::Duration};

use cosmrs::rpc::endpoint::broadcast::tx_commit::Response;
use tokio::{
    spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::{interval, sleep, Instant},
};
use tracing::{error, info, info_span, Dispatch};

use market_data_feeder::{
    configuration::{Config, Providers},
    cosmos::{
        construct_rpc_client, construct_tx, get_account_data, get_sender_account_id, Client,
        ExecuteMsg, Wallet,
    },
    error::Result,
    provider::{Factory, Provider, Type},
};

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub const MAX_SEQ_ERRORS: u8 = 5;

pub const MAX_SEQ_ERRORS_SLEEP_DURATION: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> Result<()> {
    tracing::dispatcher::set_global_default(Dispatch::new(
        tracing_subscriber::fmt()
            .with_level(true)
            .with_ansi(true)
            .with_file(true)
            .with_line_number(true)
            .with_max_level({
                #[cfg(debug_assertions)]
                {
                    tracing::level_filters::LevelFilter::DEBUG
                }
                #[cfg(not(debug_assertions))]
                {
                    tracing::level_filters::LevelFilter::INFO
                }
            })
            .finish(),
    ))
    .expect("Couldn't register global default tracing dispatcher!");

    let wallet = {
        println!("Enter feeder's account secret: ");
        let mut secret = String::new();
        io::stdin().read_line(&mut secret)?;

        Wallet::new(secret.trim(), DEFAULT_COSMOS_HD_PATH)?
    };

    info!("Successfully derived private key.");

    let cfg = read_config().unwrap_or_else(|err| {
        error!("Can not read config file: {}", err);

        exit(1);
    });

    info!("Successfully read configuration file.");

    let client = Arc::new(Client::new(cfg.oracle.clone())?);
    let oracle = Arc::new(cfg.oracle);

    info!("Fetching account data from network...");

    let sender_account_id = get_sender_account_id(&wallet, &oracle)?;
    let mut account_data = get_account_data(&client, &sender_account_id).await?;

    let rpc_client = Arc::new(construct_rpc_client(&oracle)?);

    info!("Starting workers...");

    let tick_time = Duration::from_secs(cfg.tick_time);

    let (mut set, mut receiver) = spawn_workers(&client, cfg.providers, tick_time);

    info!("Workers started. Entering broadcasting loop...");

    while let Some((instant, data)) = receiver.recv().await {
        if Instant::now().duration_since(instant) < tick_time {
            let tx_raw = construct_tx(&sender_account_id, &account_data, &wallet, &oracle, data)?;

            match tx_raw.broadcast_commit(rpc_client.as_ref()).await {
                Ok(response) => {
                    if response.check_tx.code.is_ok() {
                        account_data.sequence += 1;
                    } else {
                        account_data =
                            get_account_data(client.as_ref(), &sender_account_id).await?;
                    }

                    print_tx_response(&response);
                }
                Err(error) => error!(
                    context = %error,
                    "Error occurred while trying to broadcast transaction!"
                ),
            }
        }
    }

    while set.join_next().await.is_some() {}

    Ok(())
}

fn spawn_workers(
    client: &Arc<Client>,
    providers: Vec<Providers>,
    tick_time: Duration,
) -> (JoinSet<Result<()>>, UnboundedReceiver<(Instant, String)>) {
    let mut set = JoinSet::new();

    let (sender, receiver) = unbounded_channel();

    providers
        .into_iter()
        .map(|provider_cfg| {
            let p_type = Type::from_str(&provider_cfg.main_type).unwrap_or_else(|()| {
                error!("Unknown provider type {}", &provider_cfg.main_type);

                exit(1);
            });

            Factory::new_provider(&p_type, &provider_cfg).unwrap_or_else(|err| {
                error!("Can not create provider instance {:?}", err);

                exit(1);
            })
        })
        .enumerate()
        .for_each(|(monotonic_id, provider)| {
            let client = client.clone();

            let sender = sender.clone();

            let provider_name = format!("Provider #{}/\"{}\"", monotonic_id, provider.name());

            set.spawn(provider_main_loop(
                provider,
                client,
                sender,
                provider_name,
                tick_time,
            ));
        });

    (set, receiver)
}

async fn provider_main_loop(
    provider: Box<dyn Provider + Send>,
    client: Arc<Client>,
    sender: UnboundedSender<(Instant, String)>,
    provider_name: String,
    tick_time: Duration,
) -> Result<()> {
    let provider = { provider };

    let mut interval = interval(tick_time);

    let mut seq_error_counter = 0_u8;

    loop {
        interval.tick().await;

        let f = provider.get_spot_prices(&client);

        match f.await {
            Ok(prices) => {
                seq_error_counter = 0;

                let price_feed_json =
                    serde_json_wasm::to_string(&ExecuteMsg::FeedPrices { prices })?;

                if sender.send((Instant::now(), price_feed_json)).is_err() {
                    info!(
                        provider_name = %provider_name,
                        "Feed broadcasting has been stopped! Exiting task..."
                    );

                    return Ok(());
                }
            }
            Err(error) => {
                error!(
                    provider_name = %provider_name,
                    "Couldn't get price feed! Context: {:?}",
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

fn print_tx_response(tx_commit_response: &Response) {
    let tx_span = info_span!("Tx");

    tx_span.in_scope(|| {
        info!("{}", tx_commit_response.hash);
        info!(
            "deliver_tx.gas_used {}",
            tx_commit_response.deliver_tx.gas_used
        );
        info!("check_tx.gas_used {}", tx_commit_response.check_tx.gas_used);
        info!("deliver_tx.log {}", tx_commit_response.deliver_tx.log);
        info!("check_tx.log {}", tx_commit_response.check_tx.log);
    });
}

fn read_config() -> io::Result<Config> {
    std::fs::read_to_string("market-data-feeder.toml")
        .and_then(|content| toml::from_str(&content).map_err(Into::into))
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Dropped(pub usize);
