use std::{collections::BTreeMap, future::Future, sync::LazyLock};

use anyhow::{Context as _, Result};
use tonic::codegen::http::uri::PathAndQuery;

use chain_ops::node;

use crate::{
    oracle::Oracle,
    provider::{Amount, Base, CurrencyPair, Decimal, Provider, Quote},
};

use super::{Osmosis, SpotPriceRequest, SpotPriceResponse};

impl Osmosis {
    pub fn new() -> Self {
        static SINGLETON: LazyLock<PathAndQuery> = LazyLock::new(|| {
            PathAndQuery::from_static(
                "/osmosis.poolmanager.v2.Query/SpotPriceV2",
            )
        });

        Self {
            path_and_query: &SINGLETON,
        }
    }

    pub(super) fn normalized_price(
        spot_price: &str,
        mut base_decimal_digits: u8,
        mut quote_decimal_digits: u8,
    ) -> Result<(Amount<Base>, Amount<Quote>)> {
        let mut trimmed_spot_price = spot_price.trim_end_matches('0');

        (base_decimal_digits, quote_decimal_digits) = {
            let mut decimal_places: u8 = 36;

            decimal_places -=
                u8::try_from(spot_price.len() - trimmed_spot_price.len())?;

            (
                decimal_places,
                decimal_places
                    .wrapping_add(quote_decimal_digits)
                    .wrapping_sub(base_decimal_digits),
            )
        };

        trimmed_spot_price = trimmed_spot_price.trim_start_matches('0');

        if base_decimal_digits > 38 {
            let excess = base_decimal_digits - 38;

            trimmed_spot_price = trimmed_spot_price
                .get(..trimmed_spot_price.len().saturating_sub(excess.into()))
                .filter(|trimmed_spot_price| !trimmed_spot_price.is_empty())
                .context(
                    "Base currency's amount exceeds quote currency's amount!",
                )?;

            base_decimal_digits = 38;

            quote_decimal_digits = quote_decimal_digits
                .checked_sub(excess)
                .context("Pair exceeds allowed exponent difference of 38.")?;
        }

        if trimmed_spot_price.len() > 38 {
            let excess = u8::try_from(trimmed_spot_price.len() - 38).context(
                "Quote currency's amount length exceeds 293 bytes in length!",
            )?;

            trimmed_spot_price = &trimmed_spot_price[..38];

            base_decimal_digits =
                base_decimal_digits.checked_sub(excess).context("")?;

            quote_decimal_digits =
                quote_decimal_digits.checked_sub(excess).context("")?;
        }

        10_u128
            .checked_pow(base_decimal_digits.into())
            .context(
                "Failed to calculate price base amount due to an overflow \
                during exponentiation!",
            )
            .map(|base_amount| {
                (
                    Amount::new(Decimal::new(
                        base_amount.to_string(),
                        base_decimal_digits,
                    )),
                    Amount::new(Decimal::new(
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
            .map(|((base, quote), pool)| ((quote, base), pool))
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
    ) -> impl Future<Output = Result<(Amount<Base>, Amount<Quote>)>> + Send + 'static
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

pub struct QueryMessage {
    request: SpotPriceRequest,
    base_decimal_digits: u8,
    quote_decimal_digits: u8,
}
