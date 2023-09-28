use std::collections::BTreeMap;

use bnum::BUint;

use crate::{
    config::Ticker,
    price::{Coin, Price},
    provider::PriceComparisonGuardError,
};

/// Capable of storing integers with precision of 320 bits.
pub(crate) type UInt = BUint<5>;

pub(crate) async fn compare_prices(
    prices: &[Price],
    comparison_prices: &[Price],
    max_deviation_exclusive: u64,
) -> Result<(), PriceComparisonGuardError> {
    const HUNDRED: UInt = UInt::from_digit(100);

    const fn to_big_uint(n: u128) -> UInt {
        // Order is documented to be in Little-Endian.
        UInt::from_digits([n as u64, (n >> u64::BITS) as u64, 0, 0, 0])
    }

    let mut map: BTreeMap<Ticker, BTreeMap<Ticker, (u128, u128)>> = BTreeMap::new();

    for (price, inverted) in comparison_prices
        .iter()
        .flat_map(|price: &Price| [(price, false), (price, true)])
    {
        let (base, quote): (&Coin, &Coin) = if inverted {
            (price.amount_quote(), price.amount())
        } else {
            (price.amount(), price.amount_quote())
        };

        if map
            .entry(base.ticker().to_string())
            .or_default()
            .insert(quote.ticker().to_string(), (base.amount(), quote.amount()))
            .is_some()
        {
            return Err(PriceComparisonGuardError::DuplicatePrice(
                base.ticker().to_string(),
                quote.ticker().to_string(),
            ));
        }
    }

    for price in prices.iter() {
        let (comparison_base, comparison_quote): (u128, u128) = map
            .get(price.amount().ticker())
            .and_then(|map: &BTreeMap<Ticker, (u128, u128)>| map.get(price.amount_quote().ticker()))
            .copied()
            .ok_or_else(|| {
                PriceComparisonGuardError::MissingComparisonPrice(
                    price.amount().ticker().to_string(),
                    price.amount_quote().ticker().to_string(),
                )
            })?;

        /*
        CP_base    P_base     X
        ------- = -------- * ---
        CP_quote   P_quote   100

            CP_base    P_quote         CP_base * P_quote * 100
        X = -------- * ------- * 100 = -----------------------
            CP_quote   P_base             CP_quote * P_base

        Deviation = ABS(100 - X)
        */
        let percentage_of_comparison_price: UInt =
            (to_big_uint(comparison_base) * to_big_uint(price.amount_quote().amount()) * HUNDRED)
                / (to_big_uint(comparison_quote) * to_big_uint(price.amount().amount()));

        let deviation_percent: UInt = if percentage_of_comparison_price < HUNDRED {
            HUNDRED - percentage_of_comparison_price
        } else {
            percentage_of_comparison_price - HUNDRED
        };

        if deviation_percent >= UInt::from_digit(max_deviation_exclusive) {
            return Err(PriceComparisonGuardError::DeviationTooBig(
                price.amount().ticker().to_string(),
                price.amount_quote().ticker().to_string(),
                deviation_percent,
            ));
        }
    }

    Ok(())
}
