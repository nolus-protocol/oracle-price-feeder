use std::{
    collections::BTreeMap, convert::identity, future::Future, sync::Arc,
};

use anyhow::{bail, Context as _, Result};
use cosmrs::{
    proto::cosmos::base::abci::v1beta1::TxResponse,
    tendermint::abci::Code as TxCode, Gas,
};
use serde::Serialize;
use tokio::{
    select, spawn,
    sync::oneshot,
    task::{AbortHandle, JoinSet},
    time::{interval, sleep, timeout, Instant, MissedTickBehavior},
};

use chain_ops::{
    defer::Defer,
    task::{TxPackage, WithExpiration},
    task_set::TaskSet,
    tx,
};

use crate::{
    provider::{self, BaseAmount, CurrencyPair, QuoteAmount},
    task::Base,
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
            provider = %<$provider>::PROVIDER_NAME,
            protocol = %$protocol,
            $($body)+
        ))
    };
}

pub(crate) struct Provider<P>
where
    P: provider::Provider,
{
    base: Base,
    provider: P,
}

impl<P> Provider<P>
where
    P: provider::Provider,
{
    pub const fn new(base: Base, provider: P) -> Self {
        Self { base, provider }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut query_messages =
            self.provider.price_query_messages(&self.base.oracle)?;

        let mut queries_task_set = TaskSet::new();

        let mut price_collection_buffer =
            Vec::with_capacity(query_messages.len());

        let mut dex_block_height = self.get_dex_block_height().await?;

        self.spawn_query_tasks(
            &mut query_messages,
            &mut queries_task_set,
            &mut price_collection_buffer,
        )
        .await
        .context("Failed to spawn price querying tasks!")?;

        self.initial_fetch_and_print(&mut queries_task_set).await?;

        let mut fetch_delivered_set =
            Defer::new(JoinSet::new(), JoinSet::abort_all);

        let mut next_feed_interval = interval(self.base.idle_duration);

        next_feed_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut fallback_gas = 0;

        loop {
            select! {
                biased;
                Some((currency_pair, result)) = queries_task_set.join_next(),
                if !queries_task_set.is_empty() => {
                    self.handle_price_query_result(
                        &mut price_collection_buffer,
                        currency_pair,
                        result
                            .context("Failed to join back price query task!")?,
                    );

                    if queries_task_set.is_empty()
                        && !price_collection_buffer.is_empty() {
                        let _: AbortHandle = self.send_for_broadcast(
                            &price_collection_buffer,
                            fallback_gas,
                        )
                        .map(|feedback_response_rx| {
                            self.fetch_delivered(feedback_response_rx)
                        })
                        .map(|future| fetch_delivered_set.spawn(future))?;

                        price_collection_buffer.clear();
                    }
                },
                Some(result) = fetch_delivered_set.join_next(),
                if !fetch_delivered_set.is_empty() => {
                    let result = result.context(
                        "Failed to join back delivered transaction fetching \
                        task!",
                    )?;

                    fallback_gas = self.handle_fetch_delivered_result(
                        fallback_gas,
                        result,
                    )?;
                },
                _ = next_feed_interval.tick(),
                if queries_task_set.is_empty() => {
                    let new_block_height = self.get_dex_block_height().await?;

                    if dex_block_height >= new_block_height {
                        log_with_context!(error![self.base.protocol, P](
                            last_recorded = dex_block_height,
                            latest_reported = new_block_height,
                            "Dex node's latest block height didn't increment!",
                        ));

                        continue;
                    }

                    dex_block_height = new_block_height;

                    self.spawn_query_tasks(
                        &mut query_messages,
                        &mut queries_task_set,
                        &mut price_collection_buffer,
                    )
                    .await
                    .context("Failed to spawn price querying tasks!")?;
                },
            }
        }
    }

    async fn get_dex_block_height(&self) -> Result<u64> {
        let mut query_tendermint =
            self.base.dex_node_client.clone().query_tendermint();

        if query_tendermint.syncing().await? {
            bail!("Dex node reported in with syncing status!");
        }

        query_tendermint.get_latest_block().await
    }

    async fn initial_fetch_and_print(
        &mut self,
        queries_task_set: &mut TaskSet<
            CurrencyPair,
            Result<(BaseAmount, QuoteAmount)>,
        >,
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

        log!(info_span!("pre-feeding-check")).in_scope(|| {
            if !prices.is_empty() {
                log_with_context!(info![self.base.protocol, P](
                    "Collected prices:",
                ));

                for (
                    CurrencyPair { base, quote },
                    (base_amount, quote_amount),
                ) in prices
                {
                    let base_amount = base_amount.into_inner();

                    let base_decimal_places =
                        base_amount.decimal_places().into();

                    let base_amount = base_amount.into_amount();

                    let (base_whole, base_fraction) = base_amount.split_at(
                        base_amount.len().saturating_sub(base_decimal_places),
                    );

                    let quote_amount = quote_amount.into_inner();

                    let quote_decimal_places =
                        quote_amount.decimal_places().into();

                    let quote_amount = quote_amount.into_amount();

                    let (quote_whole, quote_fraction) = quote_amount.split_at(
                        quote_amount.len().saturating_sub(quote_decimal_places),
                    );

                    log!(info!(
                        "{base_whole:0>1}.\
                        {base_fraction:0<1} \
                        {base} ~ \
                        {quote_whole:0>1}.\
                        {quote_fraction:0<1} \
                        {quote}",
                        base_fraction = base_fraction.trim_end_matches('0'),
                        quote_fraction = quote_fraction.trim_end_matches('0'),
                    ));
                }

                log!(info!(""));
            }

            if !fetch_errors.is_empty() {
                log_with_context!(error![self.base.protocol, P](
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
        });

        sleep(self.base.duration_before_start).await;

        Ok(())
    }

    fn handle_price_query_result(
        &mut self,
        price_collection_buffer: &mut Vec<Price>,
        CurrencyPair { base, quote }: CurrencyPair,
        result: Result<(BaseAmount, QuoteAmount)>,
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
                log_with_context!(error![self.base.protocol, P](
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
        let mut query_tx = self.base.node_client.clone().query_tx();

        let source = self.base.source.clone();

        let timeout_duration = self.base.timeout_duration;

        let protocol = self.base.protocol.clone();

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
        self.base
            .execute_template
            .apply(&ExecuteMsg::FeedPrices {
                prices: price_collection_buffer,
            })
            .context("Failed to construct transaction's body!")
            .and_then(|tx_body| {
                let (feedback_sender, feedback_receiver) = oneshot::channel();

                self.base
                    .transaction_tx
                    .send(TxPackage {
                        tx_body,
                        source: self.base.source.clone(),
                        hard_gas_limit: self.base.hard_gas_limit,
                        fallback_gas,
                        feedback_sender,
                        expiration: WithExpiration::new(
                            Instant::now() + self.base.timeout_duration,
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
                    log_with_context!(info![self.base.protocol, P](
                        hash = %response.txhash,
                        height = %response.height,
                        "Transaction included in block successfully.",
                    ));
                } else if code.value() == tx::OUT_OF_GAS_ERROR_CODE {
                    log_with_context!(error![self.base.protocol, P](
                        hash = %response.txhash,
                        log = ?response.raw_log,
                        "Transaction failed, likely because it ran out of gas.",
                    ));
                } else {
                    log_with_context!(error![self.base.protocol, P](
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

                if fallback_gas <= self.base.hard_gas_limit {
                    log_with_context!(info![self.base.protocol, P](
                        %fallback_gas,
                        "Fallback gas adjusted.",
                    ));
                } else {
                    log_with_context!(warn![self.base.protocol, P](
                        %fallback_gas,
                        limit = %self.base.hard_gas_limit,
                        "Fallback gas exceeds gas limit per alarm! \
                        Clamping down!",
                    ));

                    fallback_gas = self.base.hard_gas_limit;
                };
            },
            Ok(None) => {},
            Err(error) => {
                log_with_context!(error![self.base.protocol, P](
                    ?error,
                    "Fetching delivered transaction failed!",
                ));
            },
        }

        Ok(fallback_gas)
    }

    async fn spawn_query_tasks(
        &mut self,
        query_messages: &mut BTreeMap<CurrencyPair, P::PriceQueryMessage>,
        task_set: &mut TaskSet<CurrencyPair, Result<(BaseAmount, QuoteAmount)>>,
        replacement_buffer: &mut Vec<Price>,
    ) -> Result<()> {
        if self
            .base
            .oracle
            .update_currencies_and_pairs()
            .await
            .context("Failed to update currencies and currency pairs")?
        {
            *query_messages =
                self.provider.price_query_messages(&self.base.oracle)?;

            let additional_capacity = query_messages
                .len()
                .saturating_sub(replacement_buffer.len());

            replacement_buffer.reserve_exact(additional_capacity);
        }

        query_messages
            .iter()
            .for_each(self.spawn_query_task(task_set));

        Ok(())
    }

    pub(crate) fn spawn_query_task<'r>(
        &'r self,
        task_set: &'r mut TaskSet<
            CurrencyPair,
            Result<(BaseAmount, QuoteAmount)>,
        >,
    ) -> impl FnMut((&CurrencyPair, &P::PriceQueryMessage)) + 'r {
        let duration = self.base.idle_duration;

        move |(currency_pair, message)| {
            let price_query = self
                .provider
                .price_query(&self.base.dex_node_client, message);

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
