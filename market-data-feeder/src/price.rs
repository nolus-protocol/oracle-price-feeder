use std::str::FromStr;

use serde::{de::Error as DeserializeError, Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::config::Ticker;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) struct Ratio {
    numerator: u128,
    denominator: u128,
}

impl Ratio {
    pub fn parse_string(mut price: String) -> Result<Self, Error> {
        let point: usize;

        let price: String = {
            point = price
                .find('.')
                .map_or(price.len(), |point: usize| -> usize {
                    price = price.trim_end_matches('0').into();

                    price.remove(point);

                    point
                });

            price
        };

        (price.len() - point)
            .try_into()
            .map_err(Error::from)
            .and_then(|exp: u32| 10_u128.checked_pow(exp).ok_or(Error::ExponentTooBig))
            .and_then(|denominator: u128| {
                price
                    .trim_start_matches('0')
                    .parse()
                    .map(|numerator: u128| Self {
                        numerator,
                        denominator,
                    })
                    .map_err(Error::ParseNumerator)
            })
    }

    pub const fn to_price(self, base: Ticker, quote: Ticker) -> Price {
        Price::new(base, self.denominator, quote, self.numerator)
    }
}

impl<'de> Deserialize<'de> for Ratio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .and_then(|price: String| Self::parse_string(price).map_err(DeserializeError::custom))
    }
}

impl FromStr for Ratio {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_string(String::from(s))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to parse numerator! Cause: {0}")]
    ParseNumerator(#[from] std::num::ParseIntError),
    #[error("Failed to convert denominator exponent! Cause: {0}")]
    ConvertInt(#[from] std::num::TryFromIntError),
    #[error("Failed to parse ratio! Denominator exponent too big!")]
    ExponentTooBig,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[must_use]
pub(crate) struct Coin {
    amount: u128,
    ticker: Ticker,
}

impl Coin {
    pub const fn new(amount: u128, ticker: String) -> Self {
        Self { amount, ticker }
    }

    pub const fn amount(&self) -> u128 {
        self.amount
    }

    pub const fn ticker(&self) -> &Ticker {
        &self.ticker
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[must_use]
pub(crate) struct Price {
    amount: Coin,
    amount_quote: Coin,
}

impl Price {
    pub const fn new(
        base_ticker: String,
        base_amount: u128,
        quote_ticker: String,
        quote_amount: u128,
    ) -> Self {
        Self::new_from_coins(
            Coin::new(base_amount, base_ticker),
            Coin::new(quote_amount, quote_ticker),
        )
    }

    pub const fn new_from_coins(amount: Coin, amount_quote: Coin) -> Self {
        Price {
            amount,
            amount_quote,
        }
    }

    pub const fn amount(&self) -> &Coin {
        &self.amount
    }

    pub const fn amount_quote(&self) -> &Coin {
        &self.amount_quote
    }
}
