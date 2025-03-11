use std::{
    collections::BTreeMap, convert::identity, fmt::Display, future::poll_fn,
    pin::pin, sync::Arc, task::Poll, time::Duration,
};

use anyhow::{Context as _, Result, bail};
use cosmrs::{
    proto::cosmos::base::abci::v1beta1::TxResponse,
    tendermint::abci::Code as TxCode,
};
use serde::Serialize;
use tokio::{
    spawn,
    sync::{Mutex, oneshot},
    task::{AbortHandle, JoinSet},
    time::{Instant, Interval, MissedTickBehavior, interval, sleep, timeout},
};

use ::tx::{TimeBasedExpiration, TxPackage};
use chain_ops::{
    node::{self, QueryTx},
    signer::Gas,
    tx::{self, ExecuteTemplate},
};
use channel::unbounded;
use contract::{Protocol, ProtocolDex, ProtocolProviderAndContracts};
use defer::Defer;
use dex::{
    CurrencyPair, Dex,
    amount::{Amount, Base, Decimal, Quote},
    providers::ProviderType,
};
use environment::ReadFromVar as _;
use task::{
    Run, RunnableState, Task, spawn_new, spawn_restarting,
    spawn_restarting_delayed,
};
use task_set::TaskSet;

use crate::{
    dex_node_grpc_var::dex_node_grpc_var,
    id::Id,
    oracle::Oracle,
    state::{self, State},
};

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "provider",
            $($body)+
        )
    };
}

macro_rules! log_with_context {
    ($macro:ident![$protocol:expr, $provider:ty]($($body:tt)+)) => {
        log!($macro!(
            provider = %<$provider>::PROVIDER_TYPE,
            protocol = %$protocol,
            $($body)+
        ))
    };
}

pub enum PriceFetcherRunnableState {
    New,
    ImmediateRestart,
    DelayedRestart(Duration),
}

pub async fn spawn_price_fetcher(
    task_set: &mut TaskSet<Id, Result<()>>,
    state: State,
    protocol: Arc<str>,
    transaction_tx: &unbounded::Sender<TxPackage<TimeBasedExpiration>>,
    runnable_state: PriceFetcherRunnableState,
) -> Result<State> {
    tracing::info!(%protocol, "Price fetcher is starting...");

    let state::PriceFetcher {
        mut admin_contract,
        dex_node_clients,
        idle_duration,
        signer_address,
        hard_gas_limit,
        query_tx,
        timeout_duration,
    } = state.price_fetcher().clone();

    let Protocol {
        network,
        provider_and_contracts,
    } = admin_contract.protocol(&protocol).await?;

    let task = TaskSpawner {
        task_set,
        name: protocol,
        idle_duration,
        transaction_tx: transaction_tx.clone(),
        signer_address: signer_address.clone(),
        hard_gas_limit,
        query_tx,
        dex_node_clients: dex_node_clients.clone(),
        timeout_duration,
        network,
        runnable_state,
    };

    match provider_and_contracts {
        ProtocolDex::Astroport(provider_and_contracts) => {
            task.spawn_with(provider_and_contracts).await
        },
        ProtocolDex::Osmosis(provider_and_contracts) => {
            task.spawn_with(provider_and_contracts).await
        },
    }
    .map(|()| state)
}

struct TaskSpawner<'r> {
    task_set: &'r mut TaskSet<Id, Result<()>>,
    name: Arc<str>,
    dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
    idle_duration: Duration,
    signer_address: Arc<str>,
    hard_gas_limit: Gas,
    transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
    query_tx: QueryTx,
    timeout_duration: Duration,
    network: String,
    runnable_state: PriceFetcherRunnableState,
}

impl TaskSpawner<'_> {
    async fn spawn_with<Dex>(
        self,
        ProtocolProviderAndContracts { provider, oracle }: ProtocolProviderAndContracts<Dex>,
    ) -> Result<()>
    where
        Dex: self::Dex<ProviderTypeDescriptor = ProviderType>,
    {
        let Self {
            task_set,
            name,
            dex_node_clients,
            idle_duration,
            signer_address,
            hard_gas_limit,
            transaction_tx,
            query_tx,
            timeout_duration,
            network,
            runnable_state,
        } = self;

        let oracle = ::oracle::Oracle::new(oracle)
            .await
            .context("Failed to connect to oracle contract!")?;

        let source =
            format!("{dex}; Protocol: {name}", dex = Dex::PROVIDER_TYPE).into();

        let dex_node_client =
            get_or_connect_dex_client(network, dex_node_clients).await?;

        let execute_template = ExecuteTemplate::new(
            (&*signer_address).into(),
            oracle.address().into(),
        );

        let oracle = Oracle::new(oracle, Duration::from_secs(15))
            .await
            .context("Failed to fetch oracle contract data!")?;

        let task = TaskWithProvider {
            protocol: name,
            source,
            query_tx,
            dex_node_client,
            duration_before_start: Duration::default(),
            execute_template,
            idle_duration,
            timeout_duration,
            hard_gas_limit,
            oracle,
            provider,
            transaction_tx,
        };

        match runnable_state {
            PriceFetcherRunnableState::New => spawn_new(task_set, task),
            PriceFetcherRunnableState::ImmediateRestart => {
                spawn_restarting(task_set, task)
            },
            PriceFetcherRunnableState::DelayedRestart(delay) => {
                spawn_restarting_delayed(task_set, task, delay)
            },
        }

        Ok(())
    }
}

async fn get_or_connect_dex_client(
    provider_network: String,
    dex_node_clients: Arc<Mutex<BTreeMap<Box<str>, node::Client>>>,
) -> Result<node::Client, anyhow::Error> {
    let client = {
        let guard = dex_node_clients.clone().lock_owned().await;

        guard.get(&*provider_network).cloned()
    };

    Ok(if let Some(client) = client {
        client
    } else {
        let client = node::Client::connect(&String::read_from_var(
            dex_node_grpc_var(provider_network.clone()),
        )?)
        .await
        .context("Failed to connect to node's gRPC endpoint!")?;

        dex_node_clients
            .lock_owned()
            .await
            .entry(provider_network.into_boxed_str())
            .or_insert(client)
            .clone()
    })
}

struct TaskWithProvider<Dex>
where
    Dex: self::Dex,
{
    protocol: Arc<str>,
    source: Arc<str>,
    query_tx: node::QueryTx,
    dex_node_client: node::Client,
    duration_before_start: Duration,
    execute_template: ExecuteTemplate,
    idle_duration: Duration,
    timeout_duration: Duration,
    hard_gas_limit: Gas,
    oracle: Oracle<Dex>,
    provider: Dex,
    transaction_tx: unbounded::Sender<TxPackage<TimeBasedExpiration>>,
}

impl<P> TaskWithProvider<P>
where
    P: Dex<ProviderTypeDescriptor: Display>,
{
    async fn price_fetcher_context(
        &self,
    ) -> Result<PriceFetcherContext<<P as Dex>::PriceQueryMessage>> {
        let query_messages = self
            .query_messages()
            .context("Failed to construct price query messages!")?;

        let dex_block_height = self
            .get_dex_block_height()
            .await
            .context("Failed to fetch DEX node's block height!")?;

        let mut price_collection_buffer = vec![];

        price_collection_buffer.reserve_exact(query_messages.len());

        Ok(PriceFetcherContext {
            query_messages,
            queries_task_set: TaskSet::new(),
            price_collection_buffer,
            dex_block_height,
        })
    }

    async fn initial_fetch_and_print(
        &mut self,
        queries_task_set: &mut QueryTasksSet,
    ) -> Result<()> {
        let mut prices = vec![];

        let mut fetch_errors = vec![];

        while let Some((currency_pair, result)) =
            queries_task_set.join_next().await
        {
            let result =
                result.context("Failed to join back price query task!")?;

            match result {
                Ok(price) => {
                    prices.push((currency_pair, price));
                },
                Err(error) => {
                    fetch_errors.push((currency_pair, error));
                },
            }
        }

        self.log_prices_and_errors(prices, fetch_errors);

        sleep(self.duration_before_start).await;

        Ok(())
    }

    fn log_prices_and_errors(
        &self,
        prices: Vec<QueryTaskResponse>,
        fetch_errors: Vec<(CurrencyPair, anyhow::Error)>,
    ) {
        log!(info_span!("pre-feeding-check")).in_scope(|| {
            if !prices.is_empty() {
                self.log_prices(prices);
            }

            if !fetch_errors.is_empty() {
                self.log_errors(fetch_errors);
            }
        });
    }

    fn log_prices(&self, prices: Vec<QueryTaskResponse>) {
        log_with_context!(info![self.protocol, P]("Collected prices:"));

        for (CurrencyPair { base, quote }, (base_amount, quote_amount)) in
            prices
        {
            log!(debug!("{base_amount:?} / {quote_amount:?}"));

            log!(info!(
                "{}",
                Self::pretty_formatted_price(
                    &base,
                    &base_amount,
                    &quote,
                    &quote_amount
                )
            ));

            log!(info!(
                "\t{{{base_amount} ~ {quote_amount}}}",
                base_amount = base_amount.as_inner().amount(),
                quote_amount = quote_amount.as_inner().amount(),
            ));
        }

        log!(info!(""));
    }

    fn log_errors(&self, fetch_errors: Vec<(CurrencyPair, anyhow::Error)>) {
        log_with_context!(error![self.protocol, P](
            "Errors which occurred while collecting prices:"
        ));

        for (CurrencyPair { base, quote }, error) in fetch_errors {
            log!(error!(
                %base,
                %quote,
                ?error,
                "Failed to fetch price!",
            ));
        }

        log!(error!(""));
    }

    fn pretty_formatted_price(
        base_ticker: &str,
        base_amount: &Amount<Base>,
        quote_ticker: &str,
        quote_amount: &Amount<Quote>,
    ) -> String {
        struct ProcessedAmount<'r> {
            whole: &'r str,
            zeroes_after_point: usize,
            fraction: &'r str,
        }

        fn process(amount: &Decimal) -> ProcessedAmount {
            let decimal_digits = amount.decimal_places().into();

            let amount = amount.amount();

            let amount_length = amount.len();

            let (whole, fraction) =
                amount.split_at(amount_length.saturating_sub(decimal_digits));

            let zeroes_after_point =
                decimal_digits.saturating_sub(amount_length);

            ProcessedAmount {
                whole,
                zeroes_after_point,
                fraction: fraction.trim_end_matches('0'),
            }
        }

        let base_amount = process(base_amount.as_inner());

        let quote_amount = process(quote_amount.as_inner());

        format!(
            "{base_whole:0>1}.{empty:0<base_zeroes$}{base_fraction:0<1} \
            {base_ticker} ~ \
            {quote_whole:0>1}.{empty:0<quote_zeroes$}{quote_fraction:0<1} \
            {quote_ticker}",
            empty = "",
            base_whole = base_amount.whole,
            base_zeroes = base_amount.zeroes_after_point,
            base_fraction = base_amount.fraction,
            quote_whole = quote_amount.whole,
            quote_zeroes = quote_amount.zeroes_after_point,
            quote_fraction = quote_amount.fraction,
        )
    }

    async fn join_query_or_delivered(
        &mut self,
        mut price_fetcher_context: PriceFetcherContext<
            <P as Dex>::PriceQueryMessage,
        >,
        fetch_delivered_set: &mut JoinSet<Result<Option<TxResponse>>>,
        fallback_gas: Gas,
    ) -> Result<(PriceFetcherContext<P::PriceQueryMessage>, Gas)> {
        enum QueryOrDelivered<Query, Delivered> {
            Query(Query),
            Delivered(Delivered),
        }

        let query_or_delivered = {
            let mut join_query =
                pin!(price_fetcher_context.queries_task_set.join_next());

            let mut join_delivered = pin!(fetch_delivered_set.join_next());

            poll_fn(move |ctx| {
                if let Poll::Ready(result) =
                    join_query.as_mut().poll(ctx).map(|result| {
                        QueryOrDelivered::Query(
                            result.unwrap_or_else(unreachable),
                        )
                    })
                {
                    Poll::Ready(result)
                } else {
                    join_delivered.as_mut().poll(ctx).map(|result| {
                        QueryOrDelivered::Delivered(
                            result.unwrap_or_else(unreachable),
                        )
                    })
                }
            })
            .await
        };

        match query_or_delivered {
            QueryOrDelivered::Query((currency_pair, result)) => self
                .handle_query_task_join_result(
                    price_fetcher_context,
                    fetch_delivered_set,
                    fallback_gas,
                    currency_pair,
                    result,
                )
                .map(|price_fetcher_context| {
                    (price_fetcher_context, fallback_gas)
                }),
            QueryOrDelivered::Delivered(result) => self
                .handle_delivered_task_join_result(fallback_gas, result)
                .map(|fallback_gas| (price_fetcher_context, fallback_gas)),
        }
    }

    async fn join_delivered_or_feed_interval_tick(
        &mut self,
        price_fetcher_context: PriceFetcherContext<
            <P as Dex>::PriceQueryMessage,
        >,
        fetch_delivered_set: &mut JoinSet<Result<Option<TxResponse>>>,
        next_feed_interval: &mut Interval,
        fallback_gas: Gas,
    ) -> Result<(PriceFetcherContext<<P as Dex>::PriceQueryMessage>, Gas)> {
        enum DeliveredOrTick<Delivered> {
            Delivered(Delivered),
            Tick,
        }

        let delivered_or_tick = {
            let mut join_delivered = pin!(fetch_delivered_set.join_next());

            let mut tick = pin!(next_feed_interval.tick());

            poll_fn(move |ctx| {
                if let Poll::Ready(result) =
                    join_delivered.as_mut().poll(ctx).map(|result| {
                        DeliveredOrTick::Delivered(
                            result.unwrap_or_else(unreachable),
                        )
                    })
                {
                    Poll::Ready(result)
                } else {
                    tick.as_mut()
                        .poll(ctx)
                        .map(|_: Instant| DeliveredOrTick::Tick)
                }
            })
            .await
        };

        if let DeliveredOrTick::Delivered(result) = delivered_or_tick {
            self.handle_delivered_task_join_result(fallback_gas, result)
                .map(|fallback_gas| (price_fetcher_context, fallback_gas))
        } else {
            self.tick_finished(price_fetcher_context).await.map(
                |price_fetcher_context| (price_fetcher_context, fallback_gas),
            )
        }
    }

    fn handle_query_task_join_result(
        &mut self,
        mut price_fetcher_context: PriceFetcherContext<
            <P as Dex>::PriceQueryMessage,
        >,
        fetch_delivered_set: &mut JoinSet<Result<Option<TxResponse>>>,
        fallback_gas: Gas,
        currency_pair: CurrencyPair,
        result: Result<
            Result<(Amount<Base>, Amount<Quote>)>,
            tokio::task::JoinError,
        >,
    ) -> Result<PriceFetcherContext<<P as Dex>::PriceQueryMessage>> {
        self.handle_price_query_result(
            &mut price_fetcher_context.price_collection_buffer,
            currency_pair,
            result.context("Failed to join back price query task!")?,
        );

        if price_fetcher_context.queries_task_set.is_empty()
            && !price_fetcher_context.price_collection_buffer.is_empty()
        {
            let feedback_response_rx = self
                .send_for_broadcast(
                    &price_fetcher_context.price_collection_buffer,
                    fallback_gas,
                )
                .context("Failed to send prices for broadcast!")?;

            let _abort_handle: AbortHandle = fetch_delivered_set
                .spawn(self.fetch_delivered(feedback_response_rx));

            price_fetcher_context.price_collection_buffer.clear();
        }

        Ok(price_fetcher_context)
    }

    fn handle_delivered_task_join_result(
        &self,
        fallback_gas: Gas,
        result: Result<Result<Option<TxResponse>>, tokio::task::JoinError>,
    ) -> Result<Gas> {
        self.handle_fetch_delivered_result(
            fallback_gas,
            result.context(
                "Failed to join back delivered transaction fetching task!",
            )?,
        )
        .context("Failed to process delivered transaction result!")
    }

    async fn tick_finished(
        &mut self,
        mut price_fetcher_context: PriceFetcherContext<P::PriceQueryMessage>,
    ) -> Result<PriceFetcherContext<P::PriceQueryMessage>> {
        let new_block_height = self
            .get_dex_block_height()
            .await
            .context("Failed to fetch DEX node's block height")?;

        if price_fetcher_context.dex_block_height < new_block_height {
            price_fetcher_context.dex_block_height = new_block_height;

            self.spawn_query_tasks(price_fetcher_context)
                .await
                .context("Failed to spawn price querying tasks!")
        } else {
            log_with_context!(error![self.protocol, P](
                last_recorded = price_fetcher_context.dex_block_height,
                latest_reported = new_block_height,
                "DEX node's latest block height didn't increment!",
            ));

            Ok(price_fetcher_context)
        }
    }

    fn query_messages(
        &self,
    ) -> Result<BTreeMap<CurrencyPair, <P as Dex>::PriceQueryMessage>> {
        self.provider.price_query_messages_with_associated_data(
            self.oracle
                .currency_pairs()
                .iter()
                .map(|(pair, associated_data)| (pair.clone(), associated_data)),
            self.oracle.currencies(),
        )
    }

    async fn get_dex_block_height(&self) -> Result<u64> {
        let mut query_tendermint =
            self.dex_node_client.clone().query_tendermint();

        if query_tendermint
            .syncing()
            .await
            .context("Failed to fetch DEX node's syncing status!")?
        {
            bail!("DEX node reported in with syncing status!");
        }

        query_tendermint
            .get_latest_block()
            .await
            .context("Failed to fetch DEX node's block height!")
    }

    fn handle_price_query_result(
        &mut self,
        price_collection_buffer: &mut Vec<Price>,
        CurrencyPair { base, quote }: CurrencyPair,
        result: Result<(Amount<Base>, Amount<Quote>)>,
    ) {
        match result {
            Ok((base_amount, quote_amount)) => {
                price_collection_buffer.push(Price {
                    amount: Coin {
                        amount: base_amount.into_inner().into_amount(),
                        ticker: base,
                    },
                    amount_quote: Coin {
                        amount: quote_amount.into_inner().into_amount(),
                        ticker: quote,
                    },
                });
            },
            Err(error) => {
                log_with_context!(error![self.protocol, P](
                    ?error,
                    "Price fetching failed!",
                ));
            },
        }
    }

    fn fetch_delivered(
        &self,
        feedback_response_rx: oneshot::Receiver<TxResponse>,
    ) -> impl Future<Output = Result<Option<TxResponse>>> + Send + 'static {
        let mut query_tx = self.query_tx.clone();

        let source = self.source.clone();

        let timeout_duration = self.timeout_duration;

        let protocol = self.protocol.clone();

        async move {
            let response = feedback_response_rx.await?;

            if TxCode::from(response.code).is_ok() {
                tx::fetch_delivered(
                    &mut query_tx,
                    &source,
                    response,
                    timeout_duration,
                )
                .await
            } else {
                log_with_context!(error![protocol, P](
                    hash = %response.txhash,
                    log = ?response.raw_log,
                    "Transaction failed upon broadcast!",
                ));

                Ok(None)
            }
        }
    }

    fn send_for_broadcast(
        &mut self,
        price_collection_buffer: &Vec<Price>,
        fallback_gas: Gas,
    ) -> Result<oneshot::Receiver<TxResponse>> {
        self.execute_template
            .apply(&ExecuteMsg::FeedPrices {
                prices: price_collection_buffer,
            })
            .context("Failed to construct transaction's body!")
            .and_then(|tx_body| {
                let (feedback_sender, feedback_receiver) = oneshot::channel();

                self.transaction_tx
                    .send(TxPackage {
                        tx_body,
                        source: self.source.clone(),
                        hard_gas_limit: self.hard_gas_limit,
                        fallback_gas,
                        feedback_sender,
                        expiration: TimeBasedExpiration::new(
                            Instant::now() + self.timeout_duration,
                        ),
                    })
                    .map(|()| feedback_receiver)
                    .context("Failed to send transaction for broadcasting!")
            })
    }

    fn handle_fetch_delivered_result(
        &self,
        mut fallback_gas: Gas,
        result: Result<Option<TxResponse>>,
    ) -> Result<Gas> {
        match result {
            Ok(Some(response)) => 'transaction_result_available: {
                let code: TxCode = response.code.into();

                if code.is_ok() {
                    log_with_context!(info![self.protocol, P](
                        hash = %response.txhash,
                        height = %response.height,
                        "Transaction included in block successfully.",
                    ));
                } else if code.value() == tx::OUT_OF_GAS_ERROR_CODE {
                    log_with_context!(error![self.protocol, P](
                        hash = %response.txhash,
                        log = ?response.raw_log,
                        "Transaction failed, likely because it ran out of gas.",
                    ));
                } else {
                    log_with_context!(error![self.protocol, P](
                        hash = %response.txhash,
                        log = ?response.raw_log,
                        "Transaction failed because of unknown reason!",
                    ));

                    break 'transaction_result_available;
                }

                fallback_gas = tx::adjust_fallback_gas(
                    fallback_gas,
                    response.gas_used.unsigned_abs(),
                )?;

                if fallback_gas <= self.hard_gas_limit {
                    log_with_context!(info![self.protocol, P](
                        %fallback_gas,
                        "Fallback gas adjusted.",
                    ));
                } else {
                    log_with_context!(warn![self.protocol, P](
                        %fallback_gas,
                        limit = %self.hard_gas_limit,
                        "Fallback gas exceeds gas limit per alarm! \
                        Clamping down!",
                    ));

                    fallback_gas = self.hard_gas_limit;
                }
            },
            Ok(None) => { /* TODO */ },
            Err(error) => {
                log_with_context!(error![self.protocol, P](
                    ?error,
                    "Fetching delivered transaction failed!",
                ));
            },
        }

        Ok(fallback_gas)
    }

    async fn spawn_query_tasks(
        &mut self,
        mut price_fetcher_context: PriceFetcherContext<P::PriceQueryMessage>,
    ) -> Result<PriceFetcherContext<P::PriceQueryMessage>> {
        if self
            .oracle
            .update_currencies_and_pairs()
            .await
            .context("Failed to update currencies and currency pairs")?
        {
            price_fetcher_context.query_messages = self
                .provider
                .price_query_messages_with_associated_data(
                    self.oracle.currency_pairs().iter().map(
                        |(currency_pair, associated_data)| {
                            (currency_pair.clone(), associated_data)
                        },
                    ),
                    self.oracle.currencies(),
                )
                .context("Failed to construct price query messages!")?;

            let additional_capacity =
                price_fetcher_context.query_messages.len().saturating_sub(
                    price_fetcher_context.price_collection_buffer.len(),
                );

            price_fetcher_context
                .price_collection_buffer
                .reserve_exact(additional_capacity);
        }

        price_fetcher_context.query_messages.iter().for_each(
            self.spawn_query_task(&mut price_fetcher_context.queries_task_set),
        );

        Ok(price_fetcher_context)
    }

    pub(crate) fn spawn_query_task<'r>(
        &'r self,
        task_set: &'r mut QueryTasksSet,
    ) -> impl FnMut((&CurrencyPair, &P::PriceQueryMessage)) + 'r {
        let duration = self.idle_duration;

        move |(currency_pair, message)| {
            let price_query =
                self.provider.price_query(&self.dex_node_client, message);

            task_set.add_handle(
                currency_pair.clone(),
                spawn({
                    async move {
                        timeout(duration, price_query)
                            .await
                            .context(
                                "Failed to query price before new querying \
                                period starts!",
                            )
                            .and_then(identity)
                    }
                }),
            );
        }
    }
}

impl<P> Run for TaskWithProvider<P>
where
    P: Dex<ProviderTypeDescriptor: Display>,
{
    async fn run(mut self, state: RunnableState) -> Result<()> {
        let mut price_fetcher_context = self.price_fetcher_context().await?;

        if matches!(state, RunnableState::New) {
            price_fetcher_context = self
                .spawn_query_tasks(price_fetcher_context)
                .await
                .context("Failed to spawn price querying tasks!")?;

            self.initial_fetch_and_print(
                &mut price_fetcher_context.queries_task_set,
            )
            .await
            .context(
                "Failed to fetch and print prices in initialization phase!",
            )?;
        }

        let mut fetch_delivered_set =
            Defer::new(JoinSet::new(), JoinSet::abort_all);

        let fetch_delivered_set = fetch_delivered_set.as_mut();

        let mut next_feed_interval = interval(self.idle_duration);

        next_feed_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut fallback_gas = 0;

        loop {
            match (
                price_fetcher_context.queries_task_set.is_empty(),
                fetch_delivered_set.is_empty(),
            ) {
                (false, false) => {
                    (price_fetcher_context, fallback_gas) = self
                        .join_query_or_delivered(
                            price_fetcher_context,
                            fetch_delivered_set,
                            fallback_gas,
                        )
                        .await?;
                },
                (false, true) => {
                    let (currency_pair, result) = price_fetcher_context
                        .queries_task_set
                        .join_next()
                        .await
                        .unwrap_or_else(unreachable);

                    price_fetcher_context = self
                        .handle_query_task_join_result(
                            price_fetcher_context,
                            fetch_delivered_set,
                            fallback_gas,
                            currency_pair,
                            result,
                        )?;
                },
                (true, false) => {
                    (price_fetcher_context, fallback_gas) = self
                        .join_delivered_or_feed_interval_tick(
                            price_fetcher_context,
                            fetch_delivered_set,
                            &mut next_feed_interval,
                            fallback_gas,
                        )
                        .await?;
                },
                (true, true) => {
                    next_feed_interval.tick().await;

                    price_fetcher_context =
                        self.tick_finished(price_fetcher_context).await?;
                },
            }
        }
    }
}

impl<P> Task<Id> for TaskWithProvider<P>
where
    P: Dex<ProviderTypeDescriptor: Display>,
{
    #[inline]
    fn id(&self) -> Id {
        Id::PriceFetcher {
            protocol: self.protocol.clone(),
        }
    }
}

#[cold]
#[inline]
fn unreachable<T>() -> T {
    unreachable!();
}

struct PriceFetcherContext<QueryMessage> {
    query_messages: BTreeMap<CurrencyPair, QueryMessage>,
    queries_task_set: QueryTasksSet,
    price_collection_buffer: Vec<Price>,
    dex_block_height: u64,
}

type QueryTasksSet =
    TaskSet<CurrencyPair, Result<(Amount<Base>, Amount<Quote>)>>;

type QueryTaskResponse = (CurrencyPair, (Amount<Base>, Amount<Quote>));

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ExecuteMsg<'r> {
    FeedPrices { prices: &'r [Price] },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct Price {
    amount: Coin,
    amount_quote: Coin,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct Coin {
    amount: String,
    ticker: Arc<str>,
}

#[test]
fn test_pretty_price_formatting() {
    use std::borrow::Borrow;

    use chain_ops::node;
    use dex::Currencies;

    enum Never {}

    struct Dummy;

    impl Dex for Dummy {
        type ProviderTypeDescriptor = &'static str;

        type AssociatedPairData = ();

        type PriceQueryMessage = Never;

        const PROVIDER_TYPE: &'static str = "Dummy";

        fn price_query_messages_with_associated_data<
            Pairs,
            Ticker,
            AssociatedPairData,
        >(
            &self,
            _pairs: Pairs,
            _currencies: &Currencies,
        ) -> Result<BTreeMap<CurrencyPair<Ticker>, Self::PriceQueryMessage>>
        where
            Pairs:
                IntoIterator<Item = (CurrencyPair<Ticker>, AssociatedPairData)>,
            Ticker: Borrow<str> + Ord,
            AssociatedPairData: Borrow<Self::AssociatedPairData>,
        {
            Ok(BTreeMap::new())
        }

        #[allow(clippy::manual_async_fn)]
        fn price_query(
            &self,
            _: &node::Client,
            _: &Self::PriceQueryMessage,
        ) -> impl Future<Output = Result<(Amount<Base>, Amount<Quote>)>>
        + Send
        + 'static {
            async move {
                unreachable!();
            }
        }
    }

    let base = Amount::new(Decimal::new("100000000000000000".into(), 17));

    let quote = Amount::new(Decimal::new("1811002280600015".into(), 17));

    assert_eq!(
        TaskWithProvider::<Dummy>::pretty_formatted_price(
            "NLS",
            &base,
            "USDC_NOBLE",
            &quote,
        ),
        "1.0 NLS ~ 0.01811002280600015 USDC_NOBLE"
    );

    let base = Amount::new(Decimal::new("10000000000000000000".into(), 19));

    let quote = Amount::new(Decimal::new("67247624153".into(), 7));

    assert_eq!(
        TaskWithProvider::<Dummy>::pretty_formatted_price(
            "WETH", &base, "OSMO", &quote,
        ),
        "1.0 WETH ~ 6724.7624153 OSMO"
    );
}
