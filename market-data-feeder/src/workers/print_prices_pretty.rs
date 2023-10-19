use std::cmp;

use tracing::{info, info_span};

use crate::{
    price::{Coin as _, CoinWithDecimalPlaces, Price},
    provider::Provider,
};

pub(super) fn print<P>(provider: &P, prices: &[Price<CoinWithDecimalPlaces>])
where
    P: Provider,
{
    info_span!("Prices comparison guard", provider = provider.instance_id())
        .in_scope(|| do_print(prices));
}

fn do_print(prices: &[Price<CoinWithDecimalPlaces>]) {
    info!("Prices to be fed:");

    let mut max_base_denom_width: usize = 0;
    let mut max_quote_whole_width: usize = 0;
    let mut max_quote_fraction_width: usize = 0;

    let mut prices: Vec<(&Price<CoinWithDecimalPlaces>, String, String)> = prices
        .iter()
        .map(|price: &Price<CoinWithDecimalPlaces>| {
            if max_base_denom_width < price.amount().ticker().len() {
                max_base_denom_width = price.amount().ticker().len();
            }

            let quote_amount: String = normalize_and_stringify_quote(price);

            let Some((quote_whole, quote_fraction)) = quote_amount.split_once('.') else {
                unreachable!()
            };

            if max_quote_whole_width < quote_whole.len() {
                max_quote_whole_width = quote_whole.len();
            }

            if max_quote_fraction_width < quote_fraction.len() {
                max_quote_fraction_width = quote_fraction.len();
            }

            (price, quote_whole.to_owned(), quote_fraction.to_owned())
        })
        .collect();

    prices.sort_unstable_by(cmp_price_tickers);

    for (price, quote_whole, quote_fraction) in prices {
        info!(
        "\t1 {base_denom:<base_denom_width$} ~ {quote_whole:>quote_whole_width$}.{quote_fraction:<quote_fraction_width$} {quote_denom}",
        base_denom = price.amount().ticker(),
        quote_whole = quote_whole,
        quote_fraction = quote_fraction,
        quote_denom = price.amount_quote().ticker(),
        base_denom_width = max_base_denom_width,
        quote_whole_width = max_quote_whole_width,
        quote_fraction_width = max_quote_fraction_width,
    );
    }
}

fn cmp_price_tickers(
    &(left_price, _, _): &(&Price<CoinWithDecimalPlaces>, String, String),
    &(right_price, _, _): &(&Price<CoinWithDecimalPlaces>, String, String),
) -> cmp::Ordering {
    left_price
        .amount_quote()
        .ticker()
        .cmp(right_price.amount_quote().ticker())
        .then_with(|| {
            left_price
                .amount()
                .ticker()
                .cmp(right_price.amount().ticker())
        })
}

fn normalize_and_stringify_quote(price: &Price<CoinWithDecimalPlaces>) -> String {
    let base_f64: f64 = (price.amount_quote().amount()
        * 10_u128.pow(
            price
                .amount()
                .decimal_places()
                .saturating_sub(price.amount_quote().decimal_places())
                .into(),
        )) as f64;

    let quote_f64: f64 = (price.amount().amount()
        * 10_u128.pow(
            price
                .amount_quote()
                .decimal_places()
                .saturating_sub(price.amount().decimal_places())
                .into(),
        )) as f64;

    format!(
        "{quote:.decimal_places$}",
        quote = base_f64 / quote_f64,
        decimal_places = price.amount_quote().decimal_places().into()
    )
}
