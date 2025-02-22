use std::{borrow::Borrow, cmp, collections::BTreeMap, future::Future};

use anyhow::{bail, Context as _, Result};
use tracing::debug;

use chain_ops::node;

use crate::{
    provider::{Amount, Base, CurrencyPair, Decimal, Dex, Quote},
    Currencies,
};

use super::{
    super::ProviderType, Osmosis, SpotPriceRequest, SpotPriceResponse,
};

impl Osmosis {
    pub(super) fn normalized_price(
        mut spot_price: &str,
        base_decimal_digits: u8,
        quote_decimal_digits: u8,
    ) -> Result<(Amount<Base>, Amount<Quote>)> {
        spot_price = spot_price.trim_start_matches('0');

        if spot_price.is_empty() {
            bail!("Spot price contains only zeroes!");
        }

        if !spot_price.as_bytes().iter().all(u8::is_ascii_digit) {
            bail!("Spot price is not valid ASCII encoded number!");
        }

        let mut base_exponent: u8 = 36;

        let mut quote_exponent: u8 = (36_u16 + u16::from(quote_decimal_digits))
            .checked_sub(u16::from(base_decimal_digits))
            .context(
                "Base currency's decimal digits count exceeds the sum of the 
                    quote currency's decimal digits count plus thirty six (36)!"
            )?
            .try_into()
            .context(
                "Quote currency's decimal digits count exceeds the allowed \
                    possible difference between it and the base currency's \
                    decimal digits count!"
            )?;

        spot_price = spot_price.trim_end_matches(|ch| {
            if ch == '0' && base_exponent != 0 && quote_exponent != 0 {
                base_exponent -= 1;

                quote_exponent -= 1;

                true
            } else {
                false
            }
        });

        if greater_than_max_quote_value(spot_price) {
            let excess = (spot_price.len() - MAX_QUOTE_VALUE.len())
                + usize::from(
                    spot_price[..MAX_QUOTE_VALUE.len()] > *MAX_QUOTE_VALUE,
                );

            spot_price = &spot_price[..spot_price.len() - excess];

            (base_exponent, quote_exponent) = excess
                .try_into()
                .ok()
                .and_then(|excess| {
                    base_exponent.checked_sub(excess).and_then(
                        |base_exponent| {
                            quote_exponent.checked_sub(excess).map(
                                |quote_exponent| {
                                    (base_exponent, quote_exponent)
                                },
                            )
                        },
                    )
                })
                .context("Spot price exceeds the maximum allowed value!")?;
        }

        debug_assert_eq!(spot_price.parse::<u128>().err(), None);

        10_u128
            .checked_pow(base_exponent.into())
            .context(
                "Failed to calculate price base amount due to an overflow \
                    during exponentiation!",
            )
            .map(|base_amount| {
                (
                    Amount::new(Decimal::new(
                        base_amount.to_string(),
                        base_exponent,
                    )),
                    Amount::new(Decimal::new(
                        spot_price.to_string(),
                        quote_exponent,
                    )),
                )
            })
    }
}

impl Dex for Osmosis {
    type ProviderTypeDescriptor = ProviderType;

    type AssociatedPairData = u64;

    type PriceQueryMessage = QueryMessage;

    const PROVIDER_TYPE: ProviderType = ProviderType::Osmosis;

    fn price_query_messages_with_associated_data<
        Pairs,
        Ticker,
        AssociatedPairData,
    >(
        &self,
        pairs: Pairs,
        currencies: &Currencies,
    ) -> Result<BTreeMap<CurrencyPair<Ticker>, Self::PriceQueryMessage>>
    where
        Pairs: IntoIterator<Item = (CurrencyPair<Ticker>, AssociatedPairData)>,
        Ticker: Borrow<str> + Ord,
        AssociatedPairData: Borrow<Self::AssociatedPairData>,
    {
        pairs
            .into_iter()
            .map(|(pair, pool_id)| {
                currencies.get(pair.base.borrow()).and_then(|base| {
                    currencies.get(pair.quote.borrow()).map(|quote| {
                        (
                            pair,
                            QueryMessage {
                                request: SpotPriceRequest {
                                    pool_id: *pool_id.borrow(),
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

            debug!(base, quote, "Unprocessed price: {spot_price}");

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

pub(crate) const MAX_QUOTE_VALUE: &str = {
    /// Represents the maximum 128-bit unsigned integer value.
    const MAX_U128_VALUE: &str = {
        use std::convert::identity;

        const WELL_KNOWN_VALUE: &str =
            "340282366920938463463374607431768211455";

        let length_assertion_predicate = (u128::MAX.ilog10()
            + if u128::MAX % 10 == 0 { 0 } else { 1 })
            as usize
            == WELL_KNOWN_VALUE.len();

        assert!(
            length_assertion_predicate,
            "Computed value differs from well-known value!"
        );

        let mut value = u128::MAX;

        let mut index = WELL_KNOWN_VALUE.len();

        while let Some(new_index) = index.checked_sub(1) {
            index = new_index;

            let expected_value = identity::<u8>(b'0') as u128 + (value % 10);

            let actual_value =
                identity::<u8>(WELL_KNOWN_VALUE.as_bytes()[index]) as u128;

            if expected_value == actual_value {
                value /= 10;
            } else {
                panic!("Computed value differs from well-known value!");
            }
        }

        WELL_KNOWN_VALUE
    };

    MAX_U128_VALUE
};

#[inline]
pub(crate) fn greater_than_max_quote_value(n: &str) -> bool {
    compare_with_max_quote_value(n).is_gt()
}

#[inline]
fn compare_with_max_quote_value(n: &str) -> cmp::Ordering {
    n.len().cmp(const { &MAX_QUOTE_VALUE.len() }).then_with(
        #[inline(always)]
        || n.cmp(MAX_QUOTE_VALUE),
    )
}

#[test]
fn max_quote_value_assertions() {
    let u128_max = u128::MAX.to_string();

    assert_eq!(MAX_QUOTE_VALUE, u128_max);

    assert_eq!(
        compare_with_max_quote_value("40282366920938463463374607431768211455"),
        cmp::Ordering::Less
    );

    assert_eq!(
        compare_with_max_quote_value("240282366920938463463374607431768211455"),
        cmp::Ordering::Less
    );
    assert_eq!(
        compare_with_max_quote_value("330282366920938463463374607431768211455"),
        cmp::Ordering::Less
    );
    assert_eq!(
        compare_with_max_quote_value("340282366920938463463374607431768211445"),
        cmp::Ordering::Less
    );
    assert_eq!(
        compare_with_max_quote_value("340282366920938463463374607431768211454"),
        cmp::Ordering::Less
    );

    assert_eq!(
        compare_with_max_quote_value(&u128_max),
        cmp::Ordering::Equal
    );
    assert_eq!(
        compare_with_max_quote_value("340282366920938463463374607431768211455"),
        cmp::Ordering::Equal
    );

    assert_eq!(
        compare_with_max_quote_value("340282366920938463463374607431768211456"),
        cmp::Ordering::Greater
    );
    assert_eq!(
        compare_with_max_quote_value("340282366920938463463374607431768211465"),
        cmp::Ordering::Greater
    );
    assert_eq!(
        compare_with_max_quote_value("350282366920938463463374607431768211455"),
        cmp::Ordering::Greater
    );
    assert_eq!(
        compare_with_max_quote_value("440282366920938463463374607431768211455"),
        cmp::Ordering::Greater
    );

    assert_eq!(
        compare_with_max_quote_value(
            "1340282366920938463463374607431768211455"
        ),
        cmp::Ordering::Greater
    );
}
