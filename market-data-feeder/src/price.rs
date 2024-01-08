use std::str::FromStr;

use serde::{de::Error as DeserializeError, Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::config::{Ticker, TickerUnsized};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) struct Ratio {
    numerator: u128,
    denominator: u128,
}

impl Ratio {
    pub const fn to_price(self, base: Ticker, quote: Ticker) -> Price<CoinWithoutDecimalPlaces> {
        Price::new(
            CoinWithoutDecimalPlaces::new(self.denominator, base),
            CoinWithoutDecimalPlaces::new(self.numerator, quote),
        )
    }

    pub const fn to_price_with_decimal_places(
        self,
        base_ticker: Ticker,
        base_decimal_places: u8,
        quote_ticker: Ticker,
        quote_decimal_places: u8,
    ) -> Price<CoinWithDecimalPlaces> {
        Price::new(
            CoinWithDecimalPlaces::new(self.denominator, base_ticker, base_decimal_places),
            CoinWithDecimalPlaces::new(self.numerator, quote_ticker, quote_decimal_places),
        )
    }
}

impl<'de> Deserialize<'de> for Ratio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        <&str>::deserialize(deserializer)
            .and_then(|price: &str| Self::from_str(price).map_err(DeserializeError::custom))
    }
}

impl FromStr for Ratio {
    type Err = Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_start_matches('0').trim_end_matches('0');

        let point: usize = s.find('.').unwrap_or(s.len());

        match s.len() - point {
            0 => s
                .parse()
                .map(|numerator| Self {
                    numerator,
                    denominator: 1,
                })
                .map_err(Error::ParseNumerator),
            exponent => {
                (exponent - 1)
                    .try_into()
                    .map_err(Error::from)
                    .and_then(|exp: u32| 10_u128.checked_pow(exp).ok_or(Error::ExponentTooBig))
                    .and_then(|denominator: u128| {
                        let result = s[point + 1..].trim_start_matches('0').parse();
                        if point == 0 {
                            result.map(Some)
                        } else {
                            result.and_then(|after_decimal: u128| {
                                s[..point].parse().map(|before_decimal: u128| {
                                    before_decimal.checked_mul(denominator).and_then(
                                        |before_decimal| before_decimal.checked_add(after_decimal),
                                    )
                                })
                            })
                        }
                        .map_err(Error::ParseNumerator)
                        .and_then(|maybe_numerator| maybe_numerator.ok_or(Error::NumeratorTooBig))
                        .map(|numerator: u128| Self {
                            numerator,
                            denominator,
                        })
                    })
            }
        }
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
    #[error("Failed to parse ratio! Numerator too big!")]
    NumeratorTooBig,
}

pub trait Coin: Send + 'static {
    fn amount(&self) -> u128;

    fn ticker(&self) -> &TickerUnsized;
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(into = "CoinDTO")]
#[must_use]
pub(crate) struct CoinWithoutDecimalPlaces {
    amount: u128,
    ticker: Ticker,
}

impl CoinWithoutDecimalPlaces {
    pub const fn new(amount: u128, ticker: String) -> Self {
        Self { amount, ticker }
    }
}

impl Coin for CoinWithoutDecimalPlaces {
    fn amount(&self) -> u128 {
        self.amount
    }

    fn ticker(&self) -> &TickerUnsized {
        &self.ticker
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(into = "CoinDTO")]
#[must_use]
pub(crate) struct CoinWithDecimalPlaces {
    amount: u128,
    ticker: Ticker,
    decimal_places: u8,
}

impl CoinWithDecimalPlaces {
    pub const fn new(amount: u128, ticker: String, decimal_places: u8) -> Self {
        Self {
            amount,
            ticker,
            decimal_places,
        }
    }

    pub const fn decimal_places(&self) -> u8 {
        self.decimal_places
    }
}

impl Coin for CoinWithDecimalPlaces {
    fn amount(&self) -> u128 {
        self.amount
    }

    fn ticker(&self) -> &TickerUnsized {
        &self.ticker
    }
}

#[derive(Serialize)]
struct CoinDTO {
    amount: String,
    ticker: Ticker,
}

impl From<CoinWithoutDecimalPlaces> for CoinDTO {
    fn from(CoinWithoutDecimalPlaces { amount, ticker }: CoinWithoutDecimalPlaces) -> Self {
        Self {
            amount: amount.to_string(),
            ticker,
        }
    }
}

impl From<CoinWithDecimalPlaces> for CoinDTO {
    fn from(CoinWithDecimalPlaces { amount, ticker, .. }: CoinWithDecimalPlaces) -> Self {
        Self {
            amount: amount.to_string(),
            ticker,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[must_use]
pub(crate) struct Price<C>
where
    C: Coin,
{
    amount: C,
    amount_quote: C,
}

impl<C> Price<C>
where
    C: Coin,
{
    pub const fn new(amount: C, amount_quote: C) -> Self {
        Price {
            amount,
            amount_quote,
        }
    }

    pub const fn amount(&self) -> &C {
        &self.amount
    }

    pub const fn amount_quote(&self) -> &C {
        &self.amount_quote
    }
}

#[cfg(test)]
#[test]
fn test_ratio_less_than_one() {
    const INPUT: &str = "0.1234";

    assert_eq!(
        INPUT.parse().ok(),
        Some(Ratio {
            numerator: 1234,
            denominator: 10000,
        })
    );
}

#[cfg(test)]
#[test]
fn test_ratio_greater_than_one() {
    const INPUT: &str = "1.234";

    assert_eq!(
        INPUT.parse().ok(),
        Some(Ratio {
            numerator: 1234,
            denominator: 1000,
        })
    );
}

#[cfg(test)]
#[test]
fn test_ratio_greater_than_ten() {
    const INPUT: &str = "12.34";

    assert_eq!(
        INPUT.parse().ok(),
        Some(Ratio {
            numerator: 1234,
            denominator: 100,
        })
    );
}

#[cfg(test)]
#[test]
fn test_ratio_greater_than_hundred() {
    const INPUT: &str = "123.4";

    assert_eq!(
        INPUT.parse().ok(),
        Some(Ratio {
            numerator: 1234,
            denominator: 10,
        })
    );
}

#[cfg(test)]
#[test]
fn test_ratio_integer() {
    const INPUT: &str = "1234";

    assert_eq!(
        INPUT.parse().ok(),
        Some(Ratio {
            numerator: 1234,
            denominator: 1,
        })
    );
}
