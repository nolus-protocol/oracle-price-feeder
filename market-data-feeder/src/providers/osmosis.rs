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

    fn normalized_price(spot_price: &str) -> Result<(BaseAmount, QuoteAmount)> {
        let mut decimal_places = 36;

        let mut trimmed_spot_price = spot_price.trim_end_matches('0');

        decimal_places -=
            u8::try_from(spot_price.len() - trimmed_spot_price.len())?;

        trimmed_spot_price = trimmed_spot_price.trim_start_matches('0');

        10_u128
            .checked_pow(decimal_places.into())
            .context(
                "Failed to calculate price base amount due to an overflow \
                during exponentiation!",
            )
            .map(|base_amount| {
                BaseAmount::new(DecimalAmount::new(
                    base_amount.to_string(),
                    decimal_places,
                ))
            })
            .map(|base_amount| {
                (
                    base_amount,
                    QuoteAmount::new(DecimalAmount::new(
                        trimmed_spot_price.into(),
                        decimal_places,
                    )),
                )
            })
    }
}

impl Provider for Osmosis {
    type PriceQueryMessage = SpotPriceRequest;

    const PROVIDER_NAME: &'static str = "Osmosis";

    fn price_query_messages(
        &self,
        oracle: &Oracle,
    ) -> Result<BTreeMap<CurrencyPair, Self::PriceQueryMessage>> {
        let get_currency_dex_denom = {
            let currencies = oracle.currencies();

            move |currency| {
                currencies
                    .get(currency)
                    .map(|currency| currency.dex_symbol.clone())
            }
        };

        oracle
            .currency_pairs()
            .iter()
            .map(|((base, quote), &pool_id)| {
                get_currency_dex_denom(base).and_then(|base_asset_denom| {
                    get_currency_dex_denom(quote).map(|quote_asset_denom| {
                        (
                            CurrencyPair {
                                base: base.clone().into(),
                                quote: quote.clone().into(),
                            },
                            SpotPriceRequest {
                                pool_id,
                                base_asset_denom,
                                quote_asset_denom,
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

        let query_message = query_message.clone();

        let path_and_query = self.path_and_query.clone();

        async move {
            Self::normalized_price(
                &query_raw
                    .raw::<_, SpotPriceResponse>(query_message, path_and_query)
                    .await
                    .context(
                        "Failed to query spot price from pool manager module!",
                    )?
                    .spot_price,
            )
            .context(
                "Failed to normalize price returned from pool manager module!",
            )
        }
    }
}

#[derive(Clone, Message)]
pub(crate) struct SpotPriceRequest {
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
        Osmosis::normalized_price("1941974700000000000000000000000000")
            .unwrap();

    assert_eq!(base.into_inner().into_amount(), "10000000000");

    assert_eq!(quote.into_inner().into_amount(), "19419747");

    let (base, quote) =
        Osmosis::normalized_price("001941974700000000000000000000000000")
            .unwrap();

    assert_eq!(base.into_inner().into_amount(), "10000000000");

    assert_eq!(quote.into_inner().into_amount(), "19419747");

    let (base, quote) =
        Osmosis::normalized_price("194197470000000000000000000000000000")
            .unwrap();

    assert_eq!(base.into_inner().into_amount(), "100000000");

    assert_eq!(quote.into_inner().into_amount(), "19419747");

    let (base, quote) =
        Osmosis::normalized_price("24602951060000000000000000000000000000")
            .unwrap();

    assert_eq!(base.into_inner().into_amount(), "100000000");

    assert_eq!(quote.into_inner().into_amount(), "2460295106");
}
