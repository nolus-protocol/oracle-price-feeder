use std::{
    collections::BTreeMap, convert::Infallible, hint::spin_loop,
    num::NonZeroU64, sync::Arc, time::Duration,
};

use thiserror::Error;
use tokio::{
    runtime::Handle,
    select,
    sync::{mpsc::error::SendError, Mutex},
    task::{block_in_place, JoinError, JoinSet},
    time::{error::Elapsed, sleep, timeout_at, Instant},
};
use tracing::{error, error_span, info, warn};

use broadcast::{
    generators::{
        CommitError, CommitErrorType, CommitResult, CommitResultReceiver,
        CommitResultSender, SpawnResult, TxRequest, TxRequestSender,
    },
    mode::NonBlocking,
    poll_delivered_tx,
};
use chain_comms::{
    client::Client as NodeClient,
    interact::{
        get_tx_response::Response as TxResponse,
        healthcheck::{error as healthcheck_error, Healthcheck},
        TxHash,
    },
    reexport::cosmrs::proto::{
        cosmwasm::wasm::v1::MsgExecuteContract, Any as ProtobufAny,
    },
};

use crate::{
    config::{
        ComparisonProvider as ComparisonProviderConfig,
        ComparisonProviderIdAndMaxDeviation, Provider as ProviderConfig,
        ProviderConfig as _,
        ProviderWithComparison as ProviderWithComparisonConfig,
    },
    error as error_mod,
    messages::ExecuteMsg,
    price::{CoinWithDecimalPlaces, Price},
    provider::{
        ComparisonProvider, FromConfig, PriceComparisonGuardError, Provider,
    },
    providers::{self, ComparisonProviderVisitor, ProviderVisitor},
    result::Result as AppResult,
};

mod print_prices_pretty;

pub(crate) struct SpawnContext {
    pub(crate) node_client: NodeClient,
    pub(crate) providers: BTreeMap<Box<str>, ProviderWithComparisonConfig>,
    pub(crate) price_comparison_providers:
        BTreeMap<Arc<str>, ComparisonProviderConfig>,
    pub(crate) tx_request_sender: TxRequestSender<NonBlocking>,
    pub(crate) signer_address: Arc<str>,
    pub(crate) hard_gas_limit: NonZeroU64,
    pub(crate) time_before_feeding: Duration,
    pub(crate) tick_time: Duration,
    pub(crate) poll_time: Duration,
}

pub fn spawn(
    SpawnContext {
        node_client,
        providers,
        price_comparison_providers,
        tx_request_sender,
        signer_address,
        hard_gas_limit,
        time_before_feeding,
        tick_time,
        poll_time,
    }: SpawnContext,
) -> AppResult<SpawnResult> {
    let mut tx_generators_set: JoinSet<Infallible> = JoinSet::new();

    let price_comparison_providers: BTreeMap<
        Arc<str>,
        Arc<Mutex<dyn ComparisonProvider>>,
    > = block_in_place(|| {
        price_comparison_providers
            .into_iter()
            .map(construct_comparison_provider_f(&node_client))
            .collect::<Result<_, _>>()
    })?;

    let mut tx_result_senders: BTreeMap<usize, CommitResultSender> =
        BTreeMap::new();

    providers
        .into_iter()
        .enumerate()
        .try_for_each(try_for_each_provider_f(TryForEachProviderContext {
            node_client,
            tx_generators_set: &mut tx_generators_set,
            tx_result_senders: &mut tx_result_senders,
            tx_request_sender,
            signer_address,
            price_comparison_providers,
            hard_gas_limit,
            time_before_feeding,
            tick_time,
            poll_time,
        }))
        .map(|()| SpawnResult::new(tx_generators_set, tx_result_senders))
}

fn construct_comparison_provider_f(
    node_client: &NodeClient,
) -> impl Fn(
    (Arc<str>, ComparisonProviderConfig),
) -> AppResult<(Arc<str>, Arc<Mutex<dyn ComparisonProvider>>)> {
    let node_client: NodeClient = node_client.clone();

    move |(id, config): (Arc<str>, ComparisonProviderConfig)| {
        if let Some(result) = providers::Providers::visit_comparison_provider(
            &config.provider.name().clone(),
            PriceComparisonProviderVisitor {
                provider_id: id.clone(),
                provider_config: config,
                node_client: &node_client,
            },
        ) {
            result
                .map(|comparison_provider| (id, comparison_provider))
                .map_err(error_mod::Application::Worker)
        } else {
            Err(error_mod::Application::UnknownPriceComparisonProviderId(id))
        }
    }
}

struct TryForEachProviderContext<'r> {
    node_client: NodeClient,
    tx_generators_set: &'r mut JoinSet<Infallible>,
    tx_result_senders: &'r mut BTreeMap<usize, CommitResultSender>,
    tx_request_sender: TxRequestSender<NonBlocking>,
    signer_address: Arc<str>,
    price_comparison_providers:
        BTreeMap<Arc<str>, Arc<Mutex<dyn ComparisonProvider>>>,
    hard_gas_limit: NonZeroU64,
    time_before_feeding: Duration,
    tick_time: Duration,
    poll_time: Duration,
}

fn try_for_each_provider_f(
    TryForEachProviderContext {
        node_client,
        tx_generators_set,
        tx_result_senders,
        tx_request_sender,
        signer_address,
        price_comparison_providers,
        hard_gas_limit,
        time_before_feeding,
        tick_time,
        poll_time,
    }: TryForEachProviderContext<'_>,
) -> impl FnMut((usize, (Box<str>, ProviderWithComparisonConfig))) -> AppResult<()>
       + '_ {
    move |(monotonic_id, (provider_id, config)): (
        usize,
        (Box<str>, ProviderWithComparisonConfig),
    )| {
        config
            .comparison
            .map(
                |ComparisonProviderIdAndMaxDeviation {
                     provider_id: comparison_provider_id,
                     max_deviation_exclusive,
                 }: ComparisonProviderIdAndMaxDeviation| {
                    if let Some(provider) = price_comparison_providers
                        .get(&comparison_provider_id) {
                        Ok((comparison_provider_id.clone(), provider.clone(), max_deviation_exclusive))
                    } else {
                        Err(error_mod::Application::UnknownPriceComparisonProviderId(
                            comparison_provider_id,
                        ))
                    }
                },
            )
            .transpose()
            .and_then(
                |price_comparison_provider| {
                    let provider_name: Arc<str> = config.provider.name().clone();

                    providers::Providers::visit_provider(
                        &provider_name,
                        TaskSpawningProviderVisitor {
                            worker_task_context: TaskContext {
                                tx_request_sender: tx_request_sender.clone(),
                                signer_address: signer_address.clone(),
                                hard_gas_limit,
                                monotonic_id,
                                tick_time,
                                poll_time,
                            },
                            node_client: &node_client,
                            tx_generators_set,
                            tx_result_senders,
                            provider_id,
                            provider_config: config.provider,
                            price_comparison_provider,
                            time_before_feeding,
                        },
                    )
                        .ok_or(error_mod::Application::UnknownProviderId(provider_name))
                        .and_then(|result: Result<(), error_mod::Worker>| result.map_err(From::from))
                },
            )
    }
}

struct TaskContext {
    tx_request_sender: TxRequestSender<NonBlocking>,
    signer_address: Arc<str>,
    hard_gas_limit: NonZeroU64,
    monotonic_id: usize,
    tick_time: Duration,
    poll_time: Duration,
}

struct PriceComparisonProviderVisitor<'r> {
    provider_id: Arc<str>,
    provider_config: ComparisonProviderConfig,
    node_client: &'r NodeClient,
}

impl<'r> ComparisonProviderVisitor for PriceComparisonProviderVisitor<'r> {
    type Return = Result<Arc<Mutex<dyn ComparisonProvider>>, error_mod::Worker>;

    fn on<P>(self) -> Self::Return
    where
        P: ComparisonProvider + FromConfig<true>,
    {
        Handle::current()
            .block_on(FromConfig::<true>::from_config(
                &self.provider_id,
                self.provider_config.provider,
                self.node_client,
            ))
            .map(|provider: P| {
                Arc::new(Mutex::new(provider))
                    as Arc<Mutex<dyn ComparisonProvider>>
            })
            .map_err(|error: P::ConstructError| {
                error_mod::Worker::InstantiatePriceComparisonProvider(
                    self.provider_id,
                    Box::new(error),
                )
            })
    }
}

type ComparisonProviderWithIdAndMaxDeviation =
    (Arc<str>, Arc<Mutex<dyn ComparisonProvider>>, u64);

struct TaskSpawningProviderVisitor<'r> {
    worker_task_context: TaskContext,
    node_client: &'r NodeClient,
    tx_generators_set: &'r mut JoinSet<Infallible>,
    tx_result_senders: &'r mut BTreeMap<usize, CommitResultSender>,
    provider_id: Box<str>,
    provider_config: ProviderConfig,
    price_comparison_provider: Option<ComparisonProviderWithIdAndMaxDeviation>,
    time_before_feeding: Duration,
}

impl<'r> ProviderVisitor for TaskSpawningProviderVisitor<'r> {
    type Return = Result<(), error_mod::Worker>;

    fn on<P>(self) -> Self::Return
    where
        P: Provider + FromConfig<false>,
    {
        let oracle_address: Arc<str> =
            self.provider_config.oracle_addr().clone();

        match Handle::current().block_on(<P as FromConfig<false>>::from_config(
            &self.provider_id,
            self.provider_config,
            self.node_client,
        )) {
            Ok(provider) => {
                let (commit_result_sender, commit_result_receiver): (
                    CommitResultSender,
                    CommitResultReceiver,
                ) = broadcast::generators::new_results_channel();

                self.tx_result_senders.insert(
                    self.worker_task_context.monotonic_id,
                    commit_result_sender,
                );

                self.tx_generators_set.spawn(perform_check_and_enter_loop(
                    ProviderWithIds {
                        provider,
                        provider_id: self.provider_id,
                    },
                    self.worker_task_context,
                    self.price_comparison_provider,
                    self.time_before_feeding,
                    self.node_client.clone(),
                    oracle_address,
                    commit_result_receiver,
                ));

                Ok(())
            },
            Err(error) => Err(error_mod::Worker::InstantiateProvider(
                self.provider_id,
                Box::new(error),
            )),
        }
    }
}

struct ProviderWithIds<P> {
    provider: P,
    provider_id: Box<str>,
}

async fn perform_check_and_enter_loop<P>(
    ProviderWithIds {
        mut provider,
        provider_id,
    }: ProviderWithIds<P>,
    worker_task_context: TaskContext,
    comparison_provider_and_deviation: Option<
        ComparisonProviderWithIdAndMaxDeviation,
    >,
    time_before_feeding: Duration,
    node_client: NodeClient,
    oracle_address: Arc<str>,
    commit_result_receiver: CommitResultReceiver,
) -> Infallible
where
    P: Provider,
{
    let result: Result<ChannelClosed, error_mod::Worker> = 'result: {
        let prices: Box<[Price<CoinWithDecimalPlaces>]> = {
            if let Err(error) =
                run_provider_healthcheck(&mut provider, &provider_id).await
            {
                break 'result Err(error_mod::Worker::ProviderHealthcheck(
                    error,
                ));
            }

            let result = provider.get_prices(false).await.map_err(|error| {
                error_mod::Worker::PriceComparisonGuard(
                    PriceComparisonGuardError::FetchPrices(error),
                )
            });

            match result {
                Ok(prices) => prices,
                Err(error) => {
                    break 'result Err(error);
                },
            }
        };

        if prices.is_empty() {
            error!(
                r#"Price list returned for provider "{provider_id}" is empty! Exiting providing task."#
            );

            break 'result Err(error_mod::Worker::EmptyPriceList);
        }

        if let Some((
            comparison_provider_id,
            comparison_provider,
            max_deviation_exclusive,
        )) = { comparison_provider_and_deviation }
        {
            if let Err(error) = compare_prices(
                provider.instance_id(),
                &prices,
                comparison_provider_id,
                comparison_provider,
                max_deviation_exclusive,
            )
            .await
            {
                break 'result Err(error);
            }
        } else {
            info!(
                r#"Provider "{provider_id}" isn't associated with a comparison provider."#
            );
        }

        print_prices_pretty::print(&provider, &{ prices });

        sleep(time_before_feeding).await;

        provider_main_loop(
            provider,
            &provider_id,
            worker_task_context,
            node_client,
            oracle_address,
            commit_result_receiver,
        )
        .await
    };

    let is_error: bool = result.is_err();

    let (error, cause): (String, String) = match result {
        Ok(output) => (format!("{output:?}"), output.to_string()),
        Err(error) => (format!("{error:?}"), error.to_string()),
    };

    loop {
        if is_error {
            error!(%provider_id, %error, "Provider task stopped! Cause: {cause}");
        } else {
            warn!(%provider_id, %error, "Provider task stopped! Cause: {cause}");
        }

        sleep(Duration::from_secs(15)).await;
    }
}

async fn compare_prices(
    provider_instance_id: &str,
    prices: &[Price<CoinWithDecimalPlaces>],
    comparison_provider_id: Arc<str>,
    comparison_provider: Arc<Mutex<dyn ComparisonProvider>>,
    max_deviation_exclusive: u64,
) -> Result<(), error_mod::Worker> {
    let mut comparison_provider = comparison_provider.lock().await;

    if let Some(healthcheck) = comparison_provider.healthcheck() {
        let result = run_comparison_provider_healthcheck(
            healthcheck,
            &comparison_provider_id,
        )
        .await;

        match result {
            Ok(()) => {},
            Err(error) => {
                return Err(error_mod::Worker::ComparisonProviderHealthcheck(
                    comparison_provider_id,
                    error,
                ));
            },
        }
    }

    comparison_provider
        .benchmark_prices(provider_instance_id, prices, max_deviation_exclusive)
        .await
        .map_err(error_mod::Worker::PriceComparisonGuard)
}

async fn run_comparison_provider_healthcheck(
    healthcheck: &mut Healthcheck,
    comparison_provider_id: &str,
) -> Result<(), healthcheck_error::Error> {
    healthcheck
        .wait_until_healthy(
            || {
                warn!(
                    "Comparison provider with id: \
                    {comparison_provider_id}, responded with \
                    syncing status."
                );
            },
            || {
                warn!(
                    "Comparison provider with id: \
                    {comparison_provider_id}, didn't respond with \
                    an incremented block height."
                );
            },
        )
        .await
}

async fn provider_main_loop<P>(
    mut provider: P,
    provider_id: &str,
    TaskContext {
        tx_request_sender,
        signer_address,
        hard_gas_limit,
        monotonic_id,
        tick_time,
        poll_time,
    }: TaskContext,
    node_client: NodeClient,
    oracle_address: Arc<str>,
    mut commit_result_receiver: CommitResultReceiver,
) -> Result<ChannelClosed, error_mod::Worker>
where
    P: Provider,
{
    let send_tx_request =
        move |message, fallback_gas_limit, hard_gas_limit, expiration| {
            tx_request_sender.send(TxRequest::<NonBlocking>::new(
                monotonic_id,
                vec![message],
                fallback_gas_limit,
                hard_gas_limit,
                expiration,
            ))
        };

    let mut fallback_gas_limit: NonZeroU64 = hard_gas_limit;

    let mut poll_delivered_tx_set: JoinSet<Option<(TxHash, TxResponse)>> =
        JoinSet::new();

    let mut next_tick: Instant = Instant::now();

    let output = 'worker_loop: loop {
        let idle_work_result: Result<ChannelClosed, Elapsed> = timeout_at(
            next_tick,
            handle_idle_work(
                &node_client,
                provider_id,
                &mut commit_result_receiver,
                &mut poll_delivered_tx_set,
                &mut fallback_gas_limit,
                tick_time,
                poll_time,
            ),
        )
        .await;

        if let Ok::<ChannelClosed, Elapsed>(channel_closed @ ChannelClosed {}) =
            idle_work_result
        {
            warn!(
                %provider_id,
                "Communication channel has been closed! Exiting worker task..."
            );

            break 'worker_loop Ok(channel_closed);
        }

        let healthcheck_result =
            run_provider_healthcheck(&mut provider, provider_id).await;

        match handle_healthcheck_result(provider_id, healthcheck_result) {
            Ok(HealthcheckOutcome::Healthy) => {
                next_tick = Instant::now() + tick_time;

                if let Some(channel_closed @ ChannelClosed {}) = feed_prices(
                    &mut provider,
                    provider_id,
                    &signer_address,
                    hard_gas_limit,
                    &oracle_address,
                    &send_tx_request,
                    next_tick,
                )
                .await?
                {
                    break 'worker_loop Ok(channel_closed);
                }
            },
            Ok(HealthcheckOutcome::CheckFailure) => {
                error!("Healthcheck failed! Retrying after delay!");

                next_tick = Instant::now() + Duration::from_secs(1);
            },
            Err(error) => {
                break 'worker_loop Err(
                    error_mod::Worker::ProviderHealthcheck(error),
                );
            },
        }
    };

    drop(provider);

    drop(signer_address);

    drop(node_client);

    drop(oracle_address);

    drop(commit_result_receiver);

    info!(%provider_id, "Joining all child tasks before exiting.");

    while poll_delivered_tx_set.join_next().await.is_some() {
        spin_loop();
    }

    output
}

async fn feed_prices<P, SendTxRequestF>(
    provider: &mut P,
    provider_id: &str,
    signer_address: &Arc<str>,
    hard_gas_limit: NonZeroU64,
    oracle_address: &Arc<str>,
    send_tx_request: SendTxRequestF,
    next_tick: Instant,
) -> Result<Option<ChannelClosed>, error_mod::Worker>
where
    P: Provider + Send,
    SendTxRequestF: Fn(
            ProtobufAny,
            NonZeroU64,
            NonZeroU64,
            Instant,
        ) -> Result<(), SendError<TxRequest<NonBlocking>>>
        + Send,
{
    match provider.get_prices(true).await {
        Ok(prices) => {
            let message: Vec<u8> =
                serde_json_wasm::to_string(&ExecuteMsg::FeedPrices { prices })?
                    .into_bytes();

            let message: ProtobufAny =
                ProtobufAny::from_msg(&MsgExecuteContract {
                    sender: signer_address.to_string(),
                    contract: oracle_address.to_string(),
                    msg: message,
                    funds: Vec::new(),
                })?;

            Ok(
                if send_tx_request(
                    message,
                    NonZeroU64::MAX,
                    hard_gas_limit,
                    next_tick,
                )
                .is_err()
                {
                    warn!(
                        %provider_id,
                        "Communication channel has been closed! \
                        Exiting worker task..."
                    );

                    Some(ChannelClosed {})
                } else {
                    None
                },
            )
        },
        Err(error) => {
            error!(
                %provider_id,
                ?error,
                "Couldn't get price feed! Cause: {error}"
            );

            Ok(None)
        },
    }
}

enum HealthcheckOutcome {
    CheckFailure,
    Healthy,
}

async fn run_provider_healthcheck<P>(
    provider: &mut P,
    provider_id: &str,
) -> Result<(), healthcheck_error::Error>
where
    P: Provider,
{
    provider
        .healthcheck()
        .wait_until_healthy(
            || {
                warn!(
                    "Provider with id: {provider_id}, responded with syncing \
                    status."
                );
            },
            || {
                warn!(
                    "Provider with id: {provider_id}, didn't respond with an \
                    incremented block height."
                );
            },
        )
        .await
}

fn handle_healthcheck_result(
    provider_id: &str,
    result: Result<(), healthcheck_error::Error>,
) -> Result<HealthcheckOutcome, healthcheck_error::Error> {
    match result {
        Ok(()) => Ok(HealthcheckOutcome::Healthy),
        Err(error) => {
            error_span!("Healthcheck", %provider_id);

            match error {
                healthcheck_error::Error::Syncing(
                    healthcheck_error::CheckSyncing::QuerySyncing(error),
                ) => {
                    error!(
                        ?error,
                        "Failure occurred due to connectivity error while \
                            fetching syncing status! Skipping iteration! \
                            Cause: {error}"
                    );

                    Ok(HealthcheckOutcome::CheckFailure)
                },
                healthcheck_error::Error::LatestBlockHeight(
                    healthcheck_error::LatestBlockHeight::LatestBlock(error),
                ) => {
                    error!(
                        ?error,
                        "Failure occurred due to connectivity error while \
                            fetching latest block! Skipping iteration! \
                            Cause: {error}"
                    );

                    Ok(HealthcheckOutcome::CheckFailure)
                },
                error => Err(error),
            }
        },
    }
}

#[derive(Debug, Error)]
#[error("Communication channel has been closed!")]
struct ChannelClosed;

async fn handle_idle_work(
    node_client: &NodeClient,
    provider_name: &str,
    commit_result_receiver: &mut CommitResultReceiver,
    poll_delivered_tx_set: &mut JoinSet<Option<(TxHash, TxResponse)>>,
    fallback_gas_limit: &mut NonZeroU64,
    tick_time: Duration,
    poll_time: Duration,
) -> ChannelClosed {
    loop {
        select! {
            maybe_result = commit_result_receiver.recv() => {
                if let Some(result) = maybe_result {
                    handle_commit_result(
                        node_client,
                        poll_delivered_tx_set,
                        result,
                        tick_time,
                        poll_time,
                    );
                } else {
                    break ChannelClosed {};
                }
            }
            Some(result) = poll_delivered_tx_set.join_next(), if !poll_delivered_tx_set.is_empty() => {
                handle_delivered_tx(provider_name, fallback_gas_limit, result);
            }
        }
    }
}

fn handle_commit_result(
    node_client: &NodeClient,
    poll_delivered_tx_set: &mut JoinSet<Option<(TxHash, TxResponse)>>,
    result: CommitResult,
    tick_time: Duration,
    poll_time: Duration,
) {
    match result {
        Ok(tx_hash) => {
            let node_client: NodeClient = node_client.clone();

            poll_delivered_tx_set.spawn(async move {
                poll_delivered_tx(
                    &node_client,
                    tick_time,
                    poll_time,
                    tx_hash.clone(),
                )
                .await
                .map(|tx| (tx_hash, tx))
            });
        },
        Err(CommitError {
            r#type,
            tx_response,
        }) => {
            error!(
                code = tx_response.code.value(),
                raw_log = tx_response.raw_log,
                info = ?tx_response.info,
                "Failed to commit transaction! Error type: {}",
                match r#type {
                    CommitErrorType::InvalidAccountSequence => "Invalid account sequence",
                    CommitErrorType::Unknown => "Unknown",
                },
            );
        },
    }
}

fn handle_delivered_tx(
    provider_name: &str,
    fallback_gas_limit: &mut NonZeroU64,
    result: Result<Option<(TxHash, TxResponse)>, JoinError>,
) {
    match result {
        Ok(Some((tx_hash, tx_result))) => {
            crate::log::tx_response(provider_name, &tx_hash, &tx_result);

            *fallback_gas_limit = update_fallback_gas_limit(
                *fallback_gas_limit,
                tx_result.gas_used,
            );
        },
        Ok(None) => {},
        Err(error) => {
            error!(
                "Task polling delivered transaction {}!",
                if error.is_panic() {
                    "panicked"
                } else if error.is_cancelled() {
                    "was cancelled"
                } else {
                    unreachable!()
                }
            );
        },
    }
}

#[inline]
fn update_fallback_gas_limit(
    fallback_gas_limit: NonZeroU64,
    gas_used: u64,
) -> NonZeroU64 {
    NonZeroU64::new({
        let (mut n, overflow): (u64, bool) =
            fallback_gas_limit.get().overflowing_add(gas_used);

        n >>= 1;

        if overflow {
            n |= 1 << (u64::BITS - 1);
        }

        n
    })
    .unwrap_or_else(
        #[cold]
        || unreachable!(),
    )
}
