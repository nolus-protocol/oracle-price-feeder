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

    let mut prices: Vec<(&Price<CoinWithDecimalPlaces>, String, String)> =
        prices
            .iter()
            .map(|price: &Price<CoinWithDecimalPlaces>| {
                assign_max(
                    &mut max_base_denom_width,
                    price.amount().ticker().len(),
                );

                let quote_amount: String = normalize_and_stringify_quote(price);

                let Some((quote_whole, quote_fraction)) =
                    quote_amount.split_once('.')
                else {
                    unreachable!()
                };

                assign_max(&mut max_quote_fraction_width, quote_fraction.len());

                let quote_whole_owned: String = if quote_whole.len() > 3 {
                    format_large(quote_whole, &mut max_quote_whole_width)
                } else {
                    if max_quote_whole_width < quote_whole.len() {
                        max_quote_whole_width = quote_whole.len();
                    }

                    quote_whole.to_owned()
                };

                (price, quote_whole_owned, quote_fraction.to_owned())
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

fn format_large(
    quote_whole: &str,
    max_quote_whole_width: &mut usize,
) -> String {
    let divided_len: usize = quote_whole.len() / 3;

    assign_max(max_quote_whole_width, quote_whole.len() + divided_len);

    let mut quote_whole_owned: String =
        String::with_capacity(quote_whole.len() + divided_len);

    let mut index: usize = quote_whole.len() % 3;

    if let leading_digits @ 1.. = index {
        quote_whole_owned.push_str(&quote_whole[..leading_digits]);

        quote_whole_owned.push(' ');
    }

    while index < quote_whole.len() {
        quote_whole_owned.push_str(&quote_whole[index..][..3]);

        index += 3;

        if index < quote_whole.len() {
            quote_whole_owned.push(' ');
        }
    }

    quote_whole_owned
}

fn assign_max(var: &mut usize, value: usize) {
    if *var < value {
        *var = value;
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

fn normalize_and_stringify_quote(
    price: &Price<CoinWithDecimalPlaces>,
) -> String {
    #[allow(clippy::cast_precision_loss)]
    let base_f64: f64 = (price.amount_quote().amount()
        * 10_u128.pow(
            price
                .amount()
                .decimal_places()
                .saturating_sub(price.amount_quote().decimal_places())
                .into(),
        )) as f64;

    #[allow(clippy::cast_precision_loss)]
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
