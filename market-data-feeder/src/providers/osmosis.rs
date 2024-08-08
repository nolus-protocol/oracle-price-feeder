use std::{collections::BTreeMap, future::Future};

use anyhow::{Context as _, Result};
use prost::Message;
use tonic::codegen::http::uri::PathAndQuery;

use chain_ops::node;

use crate::{
    oracle::Oracle,
    provider::{
        BaseAmount, CurrencyPair, DecimalAmount, Provider, QuoteAmount,
    },
};

pub(crate) struct Osmosis {
    path_and_query: PathAndQuery,
}

impl Osmosis {
    pub fn new() -> Self {
        Self {
            path_and_query: PathAndQuery::from_static(
                "/osmosis.poolmanager.v2.Query/SpotPriceV2",
            ),
        }
    }

    fn normalized_price(
        spot_price: &str,
        base_decimal_digits: u8,
        quote_decimal_digits: u8,
    ) -> Result<(BaseAmount, QuoteAmount)> {
        let mut trimmed_spot_price = spot_price.trim_end_matches('0');

        let (base_decimal_digits, quote_decimal_digits) = {
            let mut decimal_places = 36;

            decimal_places -=
                u8::try_from(spot_price.len() - trimmed_spot_price.len())?;

            (
                decimal_places.saturating_sub(
                    quote_decimal_digits.saturating_sub(base_decimal_digits),
                ),
                decimal_places.saturating_sub(
                    base_decimal_digits.saturating_sub(quote_decimal_digits),
                ),
            )
        };

        trimmed_spot_price = trimmed_spot_price.trim_start_matches('0');

        10_u128
            .checked_pow(base_decimal_digits.into())
            .context(
                "Failed to calculate price base amount due to an overflow \
                during exponentiation!",
            )
            .map(|base_amount| {
                (
                    BaseAmount::new(DecimalAmount::new(
                        base_amount.to_string(),
                        base_decimal_digits,
                    )),
                    QuoteAmount::new(DecimalAmount::new(
                        trimmed_spot_price.into(),
                        quote_decimal_digits,
                    )),
                )
            })
    }
}

impl Provider for Osmosis {
    type PriceQueryMessage = QueryMessage;

    const PROVIDER_NAME: &'static str = "Osmosis";

    fn price_query_messages(
        &self,
        oracle: &Oracle,
    ) -> Result<BTreeMap<CurrencyPair, Self::PriceQueryMessage>> {
        let currencies = oracle.currencies();

        oracle
            .currency_pairs()
            .iter()
            .map(|((base_ticker, quote_ticker), &pool_id)| {
                currencies.get(base_ticker).and_then(|base| {
                    currencies.get(quote_ticker).map(|quote| {
                        (
                            CurrencyPair {
                                base: base_ticker.clone().into(),
                                quote: quote_ticker.clone().into(),
                            },
                            QueryMessage {
                                request: SpotPriceRequest {
                                    pool_id,
                                    base_asset_denom: base.dex_symbol.clone(),
                                    quote_asset_denom: quote.dex_symbol.clone(),
                                },
                                base_decimal_digits: base.decimal_digits,
                                quote_decimal_digits: quote.decimal_digits,
                            },
                        )
                    })
                })
            })
            .collect()
    }

    fn price_query(
        &self,
        dex_node_client: &node::Client,
        query_message: &Self::PriceQueryMessage,
    ) -> impl Future<Output = Result<(BaseAmount, QuoteAmount)>> + Send + 'static
    {
        let mut query_raw = dex_node_client.clone().query_raw();

        let &Self::PriceQueryMessage {
            ref request,
            base_decimal_digits,
            quote_decimal_digits,
        } = query_message;

        let request = request.clone();

        let path_and_query = self.path_and_query.clone();

        async move {
            #[cfg(debug_assertions)]
            let (base, quote) = (
                request.base_asset_denom.clone(),
                request.quote_asset_denom.clone(),
            );

            let spot_price = query_raw
                .raw::<_, SpotPriceResponse>(request, path_and_query)
                .await
                .context(
                    "Failed to query spot price from pool manager module!",
                )?
                .spot_price;

            #[cfg(debug_assertions)]
            tracing::debug!(base, quote, "Unprocessed price: {spot_price}");

            Self::normalized_price(
                &spot_price,
                base_decimal_digits,
                quote_decimal_digits,
            )
            .context(
                "Failed to normalize price returned from pool manager \
                    module!",
            )
        }
    }
}

pub(crate) struct QueryMessage {
    request: SpotPriceRequest,
    base_decimal_digits: u8,
    quote_decimal_digits: u8,
}

#[derive(Clone, Message)]
struct SpotPriceRequest {
    #[prost(uint64, tag = "1")]
    pool_id: u64,
    #[prost(string, tag = "2")]
    base_asset_denom: String,
    #[prost(string, tag = "3")]
    quote_asset_denom: String,
}

#[derive(Message)]
struct SpotPriceResponse {
    #[prost(string, tag = "1")]
    spot_price: String,
}

#[test]
fn normalized_price() {
    let (base, quote) =
        Osmosis::normalized_price("1941974700000000000000000000000000", 6, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "10000000000");
    assert_eq!(base.decimal_places(), 10);

    let query = quote.into_inner();
    assert_eq!(query.amount(), "19419747");
    assert_eq!(query.decimal_places(), 10);

    let (base, quote) =
        Osmosis::normalized_price("001941974700000000000000000000000000", 6, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "10000000000");
    assert_eq!(base.decimal_places(), 10);

    let quote = quote.into_inner();
    assert_eq!(quote.amount(), "19419747");
    assert_eq!(quote.decimal_places(), 10);

    let (base, quote) =
        Osmosis::normalized_price("194197470000000000000000000000000000", 6, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "100000000");
    assert_eq!(base.decimal_places(), 8);

    let quote = quote.into_inner();
    assert_eq!(quote.amount(), "19419747");
    assert_eq!(quote.decimal_places(), 8);

    let (base, quote) = Osmosis::normalized_price(
        "24602951060000000000000000000000000000",
        6,
        6,
    )
    .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "100000000");
    assert_eq!(base.decimal_places(), 8);

    let quote = quote.into_inner();
    assert_eq!(quote.amount(), "2460295106");
    assert_eq!(quote.decimal_places(), 8);

    let (base, quote) =
        Osmosis::normalized_price("74167359355013376000000000000000000", 6, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "1000000000000000000");
    assert_eq!(base.decimal_places(), 18);

    let quote = quote.into_inner();
    assert_eq!(quote.amount(), "74167359355013376");
    assert_eq!(quote.decimal_places(), 18);

    let (base, quote) =
        Osmosis::normalized_price("6724762410000000000000000000", 18, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "100000000000000000");
    assert_eq!(base.decimal_places(), 17);

    let query = quote.into_inner();
    assert_eq!(query.amount(), "672476241");
    assert_eq!(query.decimal_places(), 5);

    let (base, quote) =
        Osmosis::normalized_price("6724762415000000000000000000", 18, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "1000000000000000000");
    assert_eq!(base.decimal_places(), 18);

    let query = quote.into_inner();
    assert_eq!(query.amount(), "6724762415");
    assert_eq!(query.decimal_places(), 6);

    let (base, quote) =
        Osmosis::normalized_price("6724762415300000000000000000", 18, 6)
            .unwrap();

    let base = base.into_inner();
    assert_eq!(base.amount(), "10000000000000000000");
    assert_eq!(base.decimal_places(), 19);

    let query = quote.into_inner();
    assert_eq!(query.amount(), "67247624153");
    assert_eq!(query.decimal_places(), 7);
}
