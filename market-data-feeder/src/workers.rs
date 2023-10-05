use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tokio::{
    runtime::Handle,
    select,
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        watch,
    },
    task::{block_in_place, JoinSet},
    time::{interval, sleep, Instant, Interval},
};
use tracing::{info, info_span};

use chain_comms::client::Client;

use crate::{
    config::{
        ComparisonProvider as ComparisonProviderConfig, ComparisonProviderIdAndMaxDeviation,
        Provider as ProviderConfig, ProviderConfig as _,
        ProviderWithComparison as ProviderWithComparisonConfig,
    },
    error,
    messages::ExecuteMsg,
    price::Price,
    provider::{
        ComparisonProvider, FromConfig, PriceComparisonGuardError, Provider, ProviderError,
    },
    providers::{self, ComparisonProviderVisitor, ProviderVisitor},
    result::Result as AppResult,
    UnboundedChannel,
};

const MAX_SEQ_ERRORS: u8 = 5;

const MAX_SEQ_ERRORS_SLEEP_DURATION: Duration = Duration::from_secs(60);

pub(crate) struct SpawnWorkersReturn {
    pub set: JoinSet<Result<(), error::Worker>>,
    pub receiver: UnboundedReceiver<(usize, Instant, String)>,
}

pub(crate) async fn spawn(
    nolus_node: Arc<Client>,
    providers: BTreeMap<String, ProviderWithComparisonConfig>,
    price_comparison_providers: BTreeMap<String, ComparisonProviderConfig>,
    oracle_addr: Arc<str>,
    tick_time: Duration,
    recovery_mode: watch::Receiver<bool>,
) -> AppResult<SpawnWorkersReturn> {
    let mut set: JoinSet<Result<(), error::Worker>> = JoinSet::new();

    let price_comparison_providers: BTreeMap<Arc<str>, Arc<dyn ComparisonProvider>> =
        block_in_place(|| {
            price_comparison_providers
                .into_iter()
                .map(construct_comparison_provider_f(&oracle_addr, &nolus_node))
                .collect::<Result<_, _>>()
        })?;

    let (sender, receiver): UnboundedChannel<(usize, Instant, String)> = unbounded_channel();

    block_in_place(move || {
        providers
            .into_iter()
            .enumerate()
            .try_for_each(try_for_each_provider_f(
                price_comparison_providers,
                &mut set,
                tick_time,
                recovery_mode,
                nolus_node,
                sender,
                oracle_addr,
            ))
            .map(|()| SpawnWorkersReturn { set, receiver })
    })
}

fn construct_comparison_provider_f(
    oracle_addr: &Arc<str>,
    nolus_node: &Arc<Client>,
) -> impl Fn((String, ComparisonProviderConfig)) -> AppResult<(Arc<str>, Arc<dyn ComparisonProvider>)>
{
    let nolus_node: Arc<Client> = nolus_node.clone();
    let oracle_addr: Arc<str> = oracle_addr.clone();

    move |(id, config): (String, ComparisonProviderConfig)| {
        let id: Arc<str> = id.into();

        if let Some(result) = providers::Providers::visit_comparison_provider(
            &config.provider.name().clone(),
            PriceComparisonProviderVisitor {
                provider_id: id.clone(),
                provider_config: config,
                oracle_addr: &oracle_addr,
                nolus_node: &nolus_node,
            },
        ) {
            result
                .map(|comparison_provider: Arc<dyn ComparisonProvider>| (id, comparison_provider))
                .map_err(error::Application::Worker)
        } else {
            Err(error::Application::UnknownPriceComparisonProviderId(id))
        }
    }
}

fn try_for_each_provider_f(
    price_comparison_providers: BTreeMap<Arc<str>, Arc<dyn ComparisonProvider>>,
    set: &mut JoinSet<Result<(), error::Worker>>,
    tick_time: Duration,
    recovery_mode: watch::Receiver<bool>,
    nolus_node: Arc<Client>,
    sender: UnboundedSender<(usize, Instant, String)>,
    oracle_addr: Arc<str>,
) -> impl FnMut((usize, (String, ProviderWithComparisonConfig))) -> AppResult<()> + '_ {
    move |(monotonic_id, (id, config)): (usize, (String, ProviderWithComparisonConfig))| {
        config
            .comparison
            .map(
                |ComparisonProviderIdAndMaxDeviation {
                     provider_id,
                     max_deviation_exclusive,
                 }: ComparisonProviderIdAndMaxDeviation| {
                    price_comparison_providers
                        .get(provider_id.as_str())
                        .map_or_else(
                            || {
                                Err(error::Application::UnknownPriceComparisonProviderId(
                                    provider_id.into(),
                                ))
                            },
                            |provider: &Arc<dyn ComparisonProvider>| {
                                Ok((provider, max_deviation_exclusive))
                            },
                        )
                },
            )
            .transpose()
            .and_then(
                |price_comparison_provider: Option<(&Arc<dyn ComparisonProvider>, u64)>| {
                    let provider_name: Arc<str> = config.provider.name().clone();

                    providers::Providers::visit_provider(
                        &provider_name,
                        TaskSpawningProviderVisitor {
                            worker_task_spawner_config: TaskSpawnerConfig {
                                set,
                                monotonic_id,
                                tick_time,
                                recovery_mode: &recovery_mode,
                                price_comparison_provider,
                            },
                            provider_id: &id,
                            provider_config: config.provider,
                            time_before_feeding: config.time_before_feeding,
                            nolus_node: &nolus_node,
                            sender: &sender,
                            oracle_addr: &oracle_addr,
                        },
                    )
                    .ok_or(error::Application::UnknownProviderId(provider_name))
                    .and_then(|result: Result<(), error::Worker>| result.map_err(From::from))
                },
            )
    }
}

struct TaskSpawnerConfig<'r> {
    set: &'r mut JoinSet<Result<(), error::Worker>>,
    monotonic_id: usize,
    tick_time: Duration,
    recovery_mode: &'r watch::Receiver<bool>,
    price_comparison_provider: Option<(&'r Arc<dyn ComparisonProvider>, u64)>,
}

struct PriceComparisonProviderVisitor<'r> {
    provider_id: Arc<str>,
    provider_config: ComparisonProviderConfig,
    oracle_addr: &'r Arc<str>,
    nolus_node: &'r Arc<Client>,
}

impl<'r> ComparisonProviderVisitor for PriceComparisonProviderVisitor<'r> {
    type Return = Result<Arc<dyn ComparisonProvider>, error::Worker>;

    fn on<P>(self) -> Self::Return
    where
        P: ComparisonProvider + FromConfig<true>,
    {
        Handle::current()
            .block_on(FromConfig::<true>::from_config(
                &self.provider_id,
                self.provider_config.provider,
                self.oracle_addr,
                self.nolus_node,
            ))
            .map(|provider: P| Arc::new(provider) as Arc<dyn ComparisonProvider>)
            .map_err(|error: P::ConstructError| {
                error::Worker::InstantiatePriceComparisonProvider(self.provider_id, Box::new(error))
            })
    }
}

struct TaskSpawningProviderVisitor<'r> {
    worker_task_spawner_config: TaskSpawnerConfig<'r>,
    provider_id: &'r str,
    provider_config: ProviderConfig,
    time_before_feeding: Duration,
    nolus_node: &'r Arc<Client>,
    sender: &'r UnboundedSender<(usize, Instant, String)>,
    oracle_addr: &'r Arc<str>,
}

impl<'r> ProviderVisitor for TaskSpawningProviderVisitor<'r> {
    type Return = Result<(), error::Worker>;

    fn on<P>(self) -> Self::Return
    where
        P: Provider + FromConfig<false>,
    {
        match Handle::current().block_on(<P as FromConfig<false>>::from_config(
            self.provider_id,
            self.provider_config,
            self.oracle_addr,
            self.nolus_node,
        )) {
            Ok(provider) => {
                self.worker_task_spawner_config
                    .set
                    .spawn(perform_check_and_enter_loop(
                        (
                            provider,
                            format!("Provider \"{}\" [{}]", self.provider_id, P::ID),
                        ),
                        self.worker_task_spawner_config
                            .price_comparison_provider
                            .map(|(comparison_provider, max_deviation_exclusive)| {
                                (comparison_provider.clone(), max_deviation_exclusive)
                            }),
                        self.time_before_feeding,
                        self.sender.clone(),
                        (
                            self.worker_task_spawner_config.monotonic_id,
                            self.worker_task_spawner_config.tick_time,
                        ),
                        self.worker_task_spawner_config.recovery_mode.clone(),
                    ));

                Ok(())
            }
            Err(error) => Err(error::Worker::InstantiateProvider(
                self.provider_id.to_string(),
                Box::new(error),
            )),
        }
    }
}

async fn perform_check_and_enter_loop<P>(
    (provider, provider_name): (P, String),
    comparison_provider_and_deviation: Option<(Arc<dyn ComparisonProvider>, u64)>,
    time_before_feeding: Duration,
    sender: UnboundedSender<(usize, Instant, String)>,
    (monotonic_id, tick_time): (usize, Duration),
    recovery_mode: watch::Receiver<bool>,
) -> Result<(), error::Worker>
where
    P: Provider,
{
    let prices: Box<[Price]> =
        provider
            .get_prices(false)
            .await
            .map_err(|error: ProviderError| {
                error::Worker::PriceComparisonGuard(PriceComparisonGuardError::FetchPrices(error))
            })?;

    if let Some((comparison_provider, max_deviation_exclusive)) = comparison_provider_and_deviation
    {
        comparison_provider
            .benchmark_prices(provider.instance_id(), &prices, max_deviation_exclusive)
            .await?;
    }

    info_span!("Prices comparison guard", provider = provider.instance_id()).in_scope(|| {
        info!("Prices to be fed:");

        for price in prices.iter() {
            info!(
                "\t1 {} ~ {} {:.12}",
                price.amount().ticker(),
                (price.amount_quote().amount() as f64) / (price.amount().amount() as f64),
                price.amount_quote().ticker()
            );
        }
    });

    sleep(time_before_feeding).await;

    provider_main_loop(
        provider,
        move |instant: Instant, data: String| {
            sender.send((monotonic_id, instant, data)).map_err(|_| ())
        },
        provider_name,
        tick_time,
        recovery_mode,
    )
    .await
}

async fn provider_main_loop<SenderFn, P>(
    provider: P,
    sender: SenderFn,
    provider_name: String,
    tick_time: Duration,
    mut recovery_mode: watch::Receiver<bool>,
) -> Result<(), error::Worker>
where
    SenderFn: Fn(Instant, String) -> Result<(), ()>,
    P: Provider,
{
    let mut interval: Interval = interval(tick_time);

    let mut seq_error_counter: u8 = 0;

    'worker_loop: loop {
        if select! {
            _ = interval.tick() => false,
            Ok(()) = recovery_mode.changed() => {
                *recovery_mode.borrow()
            }
        } {
            while *recovery_mode.borrow() {
                if recovery_mode.changed().await.is_err() {
                    error!("Recovery mode state watch closed! Exiting worker loop...");

                    break 'worker_loop Err(error::Worker::RecoveryModeWatchClosed);
                }
            }
        }

        match provider.get_prices(true).await {
            Ok(prices) => {
                seq_error_counter = 0;

                let price_feed_json: String =
                    serde_json_wasm::to_string(&ExecuteMsg::FeedPrices { prices })?;

                if sender(Instant::now(), price_feed_json).is_err() {
                    info!(
                        provider_name = %provider_name,
                        "Communication channel has been closed! Exiting worker task..."
                    );

                    break 'worker_loop Ok(());
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
