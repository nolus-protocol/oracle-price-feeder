use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures::executor::block_on;
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
use tracing::info;

use chain_comms::client::Client;

use crate::{
    config::{
        ComparisonProvider as ComparisonProviderConfig, ComparisonProviderIdAndMaxDeviation,
        ProviderConfig as _, ProviderWithComparison as ProviderWithComparisonConfig,
    },
    error,
    messages::ExecuteMsg,
    provider::{ComparisonProvider, FromConfig, Provider},
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
    price_comparison_providers: &BTreeMap<String, ComparisonProviderConfig>,
    oracle_addr: Arc<str>,
    tick_time: Duration,
    recovery_mode: watch::Receiver<bool>,
) -> AppResult<SpawnWorkersReturn> {
    let mut set: JoinSet<Result<(), error::Worker>> = JoinSet::new();

    let price_comparison_providers: BTreeMap<String, Arc<dyn ComparisonProvider>> =
        block_in_place(|| {
            price_comparison_providers
                .iter()
                .map(construct_comparison_provider_f(&oracle_addr, &nolus_node))
                .collect::<Result<_, _>>()
        })?;

    let (sender, receiver): UnboundedChannel<(usize, Instant, String)> = unbounded_channel();

    block_in_place(move || {
        providers
            .iter()
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
) -> impl Fn((&String, &ComparisonProviderConfig)) -> AppResult<(String, Arc<dyn ComparisonProvider>)>
{
    let nolus_node: Arc<Client> = nolus_node.clone();
    let oracle_addr: Arc<str> = oracle_addr.clone();

    move |(id, config): (&String, &ComparisonProviderConfig)| {
        providers::Providers::visit_comparison_provider(
            config.provider.name(),
            PriceComparisonProviderVisitor {
                provider_id: id,
                provider_config: config,
                oracle_addr: &oracle_addr,
                nolus_node: &nolus_node,
            },
        )
        .map_or_else(
            || {
                Err(error::Application::UnknownPriceComparisonProviderId(
                    id.clone(),
                ))
            },
            |result: Result<Arc<dyn ComparisonProvider>, error::Worker>| {
                result
                    .map(|comparison_provider: Arc<dyn ComparisonProvider>| {
                        (id.clone(), comparison_provider)
                    })
                    .map_err(error::Application::Worker)
            },
        )
    }
}

fn try_for_each_provider_f(
    price_comparison_providers: BTreeMap<String, Arc<dyn ComparisonProvider>>,
    set: &mut JoinSet<Result<(), error::Worker>>,
    tick_time: Duration,
    recovery_mode: watch::Receiver<bool>,
    nolus_node: Arc<Client>,
    sender: UnboundedSender<(usize, Instant, String)>,
    oracle_addr: Arc<str>,
) -> impl FnMut((usize, (&String, &ProviderWithComparisonConfig))) -> AppResult<()> + '_ {
    move |(monotonic_id, (id, config)): (usize, (&String, &ProviderWithComparisonConfig))| {
        config
            .comparison
            .as_ref()
            .map(
                |&ComparisonProviderIdAndMaxDeviation {
                     provider_id: ref provider,
                     max_deviation_exclusive,
                 }: &ComparisonProviderIdAndMaxDeviation| {
                    price_comparison_providers.get(provider).map_or_else(
                        || {
                            Err(error::Application::UnknownPriceComparisonProviderId(
                                provider.to_string(),
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
                    providers::Providers::visit_provider(
                        config.provider.name(),
                        TaskSpawningProviderVisitor {
                            worker_task_spawner_config: TaskSpawnerConfig {
                                set,
                                monotonic_id,
                                tick_time,
                                recovery_mode: &recovery_mode,
                                price_comparison_provider,
                            },
                            provider_id: id,
                            provider_config: config,
                            nolus_node: &nolus_node,
                            sender: &sender,
                            oracle_addr: &oracle_addr,
                        },
                    )
                    .ok_or(error::Application::UnknownProviderId(String::from(
                        config.provider.name(),
                    )))
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
    provider_id: &'r str,
    provider_config: &'r ComparisonProviderConfig,
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
                self.provider_id,
                &self.provider_config.provider,
                self.oracle_addr,
                self.nolus_node,
            ))
            .map(|provider: P| Arc::new(provider) as Arc<dyn ComparisonProvider>)
            .map_err(|error: P::ConstructError| {
                error::Worker::InstantiatePriceComparisonProvider(
                    self.provider_id.to_string(),
                    Box::new(error),
                )
            })
    }
}

struct TaskSpawningProviderVisitor<'r> {
    worker_task_spawner_config: TaskSpawnerConfig<'r>,
    provider_id: &'r str,
    provider_config: &'r ProviderWithComparisonConfig,
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
        match block_on(<P as FromConfig<false>>::from_config(
            self.provider_id,
            &self.provider_config.provider,
            self.oracle_addr,
            self.nolus_node,
        )) {
            Ok(provider) => {
                self.worker_task_spawner_config
                    .set
                    .spawn(perform_check_and_enter_loop(
                        provider,
                        self.worker_task_spawner_config
                            .price_comparison_provider
                            .map(|(comparison_provider, max_deviation_exclusive)| {
                                (comparison_provider.clone(), max_deviation_exclusive)
                            }),
                        format!("Provider \"{}\" [{}]", self.provider_id, P::ID),
                        self.sender.clone(),
                        self.worker_task_spawner_config.monotonic_id,
                        self.worker_task_spawner_config.tick_time,
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
    provider: P,
    comparison_provider_and_deviation: Option<(Arc<dyn ComparisonProvider>, u64)>,
    provider_name: String,
    sender: UnboundedSender<(usize, Instant, String)>,
    monotonic_id: usize,
    tick_time: Duration,
    recovery_mode: watch::Receiver<bool>,
) -> Result<(), error::Worker>
where
    P: Provider,
{
    if let Some((comparison_provider, max_deviation_exclusive)) = comparison_provider_and_deviation
    {
        comparison_provider
            .benchmark_prices(&provider, max_deviation_exclusive)
            .await?;
    }

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
