use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tokio::{
    runtime::Handle,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::{block_in_place, JoinSet},
    time::{interval, sleep, Instant, Interval},
};
use tracing::info;

use chain_comms::client::Client as NodeClient;

use crate::{
    config::{
        ComparisonProvider as ComparisonProviderConfig, ComparisonProviderIdAndMaxDeviation,
        Provider as ProviderConfig, ProviderConfig as _,
        ProviderWithComparison as ProviderWithComparisonConfig,
    },
    error,
    messages::ExecuteMsg,
    price::{CoinWithDecimalPlaces, Price},
    provider::{
        ComparisonProvider, FromConfig, PriceComparisonGuardError, Provider, ProviderError,
    },
    providers::{self, ComparisonProviderVisitor, ProviderVisitor},
    result::Result as AppResult,
    UnboundedChannel,
};

mod print_prices_pretty;

const MAX_SEQ_ERRORS: u8 = 5;

const MAX_SEQ_ERRORS_SLEEP_DURATION: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct PriceDataMessage {
    pub oracle: Arc<str>,
    pub execute_message: Arc<[u8]>,
}

pub(crate) struct PriceDataPacket {
    pub provider_id: usize,
    pub tx_time: Instant,
    pub message: PriceDataMessage,
}

type PriceDataSender = UnboundedSender<PriceDataPacket>;
type PriceDataReceiver = UnboundedReceiver<PriceDataPacket>;

pub(crate) struct SpawnWorkersReturn {
    pub set: JoinSet<Result<(), error::Worker>>,
    pub id_to_name_mapping: BTreeMap<usize, Arc<str>>,
    pub receiver: PriceDataReceiver,
}

pub(crate) fn spawn(
    nolus_node: NodeClient,
    providers: BTreeMap<Arc<str>, ProviderWithComparisonConfig>,
    price_comparison_providers: BTreeMap<Arc<str>, ComparisonProviderConfig>,
    tick_time: Duration,
) -> AppResult<SpawnWorkersReturn> {
    let mut set: JoinSet<Result<(), error::Worker>> = JoinSet::new();

    let price_comparison_providers: BTreeMap<Arc<str>, Arc<dyn ComparisonProvider>> =
        block_in_place(|| {
            price_comparison_providers
                .into_iter()
                .map(construct_comparison_provider_f(&nolus_node))
                .collect::<Result<_, _>>()
        })?;

    let mut id_to_name_mapping = BTreeMap::new();

    let (sender, receiver): UnboundedChannel<PriceDataPacket> = unbounded_channel();

    providers
        .into_iter()
        .enumerate()
        .try_for_each(try_for_each_provider_f(
            price_comparison_providers,
            &mut set,
            &mut id_to_name_mapping,
            tick_time,
            nolus_node,
            sender,
        ))
        .map(|()| SpawnWorkersReturn {
            set,
            id_to_name_mapping,
            receiver,
        })
}

fn construct_comparison_provider_f(
    nolus_node: &NodeClient,
) -> impl Fn((Arc<str>, ComparisonProviderConfig)) -> AppResult<(Arc<str>, Arc<dyn ComparisonProvider>)>
{
    let nolus_node: NodeClient = nolus_node.clone();

    move |(id, config): (Arc<str>, ComparisonProviderConfig)| {
        if let Some(result) = providers::Providers::visit_comparison_provider(
            &config.provider.name().clone(),
            PriceComparisonProviderVisitor {
                provider_id: id.clone(),
                provider_config: config,
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

fn try_for_each_provider_f<'r>(
    price_comparison_providers: BTreeMap<Arc<str>, Arc<dyn ComparisonProvider>>,
    set: &'r mut JoinSet<Result<(), error::Worker>>,
    id_to_name_mapping: &'r mut BTreeMap<usize, Arc<str>>,
    tick_time: Duration,
    nolus_node: NodeClient,
    price_data_sender: PriceDataSender,
) -> impl FnMut((usize, (Arc<str>, ProviderWithComparisonConfig))) -> AppResult<()> + 'r {
    move |(monotonic_id, (provider_id, config)): (
        usize,
        (Arc<str>, ProviderWithComparisonConfig),
    )| {
        config
            .comparison
            .map(
                |ComparisonProviderIdAndMaxDeviation {
                     provider_id: comparison_provider_id,
                     max_deviation_exclusive,
                 }: ComparisonProviderIdAndMaxDeviation| {
                    price_comparison_providers
                        .get(&comparison_provider_id)
                        .map_or_else(
                            || {
                                Err(error::Application::UnknownPriceComparisonProviderId(
                                    comparison_provider_id,
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
                                price_comparison_provider,
                            },
                            id_to_name_mapping,
                            provider_id,
                            provider_config: config.provider,
                            time_before_feeding: config.time_before_feeding,
                            nolus_node: &nolus_node,
                            sender: &price_data_sender,
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
    price_comparison_provider: Option<(&'r Arc<dyn ComparisonProvider>, u64)>,
}

struct PriceComparisonProviderVisitor<'r> {
    provider_id: Arc<str>,
    provider_config: ComparisonProviderConfig,
    nolus_node: &'r NodeClient,
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
    id_to_name_mapping: &'r mut BTreeMap<usize, Arc<str>>,
    provider_id: Arc<str>,
    provider_config: ProviderConfig,
    time_before_feeding: Duration,
    nolus_node: &'r NodeClient,
    sender: &'r PriceDataSender,
}

impl<'r> ProviderVisitor for TaskSpawningProviderVisitor<'r> {
    type Return = Result<(), error::Worker>;

    fn on<P>(self) -> Self::Return
    where
        P: Provider + FromConfig<false>,
    {
        let oracle_address = self.provider_config.oracle_addr().clone();

        match Handle::current().block_on(<P as FromConfig<false>>::from_config(
            &self.provider_id,
            self.provider_config,
            self.nolus_node,
        )) {
            Ok(provider) => {
                let provider_friendly_name =
                    format!("Provider \"{}\" [{}]", self.provider_id, P::ID).into_boxed_str();

                self.id_to_name_mapping.insert(
                    self.worker_task_spawner_config.monotonic_id,
                    self.provider_id.clone(),
                );

                self.worker_task_spawner_config
                    .set
                    .spawn(perform_check_and_enter_loop(
                        (provider, self.provider_id.clone(), provider_friendly_name),
                        self.worker_task_spawner_config
                            .price_comparison_provider
                            .map(|(comparison_provider, max_deviation_exclusive)| {
                                (comparison_provider.clone(), max_deviation_exclusive)
                            }),
                        self.time_before_feeding,
                        oracle_address,
                        self.sender.clone(),
                        self.worker_task_spawner_config.monotonic_id,
                        self.worker_task_spawner_config.tick_time,
                    ));

                Ok(())
            }
            Err(error) => Err(error::Worker::InstantiateProvider(
                self.provider_id,
                Box::new(error),
            )),
        }
    }
}

async fn perform_check_and_enter_loop<P>(
    (provider, provider_id, provider_friendly_name): (P, Arc<str>, Box<str>),
    comparison_provider_and_deviation: Option<(Arc<dyn ComparisonProvider>, u64)>,
    time_before_feeding: Duration,
    oracle_address: Arc<str>,
    sender: PriceDataSender,
    monotonic_id: usize,
    tick_time: Duration,
) -> Result<(), error::Worker>
where
    P: Provider,
{
    let prices: Box<[Price<CoinWithDecimalPlaces>]> =
        provider
            .get_prices(false)
            .await
            .map_err(|error: ProviderError| {
                error::Worker::PriceComparisonGuard(PriceComparisonGuardError::FetchPrices(error))
            })?;

    if prices.is_empty() {
        error!(
            r#"Price list returned for provider "{provider_friendly_name}" is empty! Exiting providing task."#
        );

        return Err(error::Worker::EmptyPriceList(provider_id));
    }

    drop(provider_id);

    if let Some((comparison_provider, max_deviation_exclusive)) = comparison_provider_and_deviation
    {
        comparison_provider
            .benchmark_prices(provider.instance_id(), &prices, max_deviation_exclusive)
            .await?;
    } else {
        info!(
            r#"Provider "{provider_friendly_name}" isn't associated with a comparison provider."#
        );
    }

    print_prices_pretty::print(&provider, &prices);

    sleep(time_before_feeding).await;

    provider_main_loop(
        provider,
        move |message: Arc<[u8]>| {
            sender
                .send(PriceDataPacket {
                    provider_id: monotonic_id,
                    tx_time: Instant::now(),
                    message: PriceDataMessage {
                        oracle: oracle_address.clone(),
                        execute_message: message,
                    },
                })
                .map_err(|_| ())
        },
        provider_friendly_name,
        tick_time,
    )
    .await
}

async fn provider_main_loop<SenderFn, P>(
    provider: P,
    sender: SenderFn,
    provider_name: Box<str>,
    tick_time: Duration,
) -> Result<(), error::Worker>
where
    SenderFn: Fn(Arc<[u8]>) -> Result<(), ()>,
    P: Provider,
{
    let mut interval: Interval = interval(tick_time);

    let mut seq_error_counter: u8 = 0;

    'worker_loop: loop {
        interval.tick().await;

        match provider.get_prices(true).await {
            Ok(prices) => {
                seq_error_counter = 0;

                if sender(
                    serde_json_wasm::to_string(&ExecuteMsg::FeedPrices { prices })?
                        .into_bytes()
                        .into(),
                )
                .is_err()
                {
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
