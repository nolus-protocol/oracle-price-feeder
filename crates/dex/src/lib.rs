use std::{borrow::Borrow, collections::BTreeMap};

use anyhow::Context;

use crate::provider::{CurrencyPair, Dex};

pub mod provider;
pub mod providers;

#[repr(transparent)]
pub struct Currencies<T = String>(BTreeMap<T, Currency>)
where
    T: Borrow<str> + Ord;

impl<T> Currencies<T>
where
    T: Borrow<str> + Ord,
{
    pub fn get(&self, currency: &str) -> anyhow::Result<&Currency> {
        self.0.get(currency).context(format!(
            "Currency {currency:?} which appears in the swap pairs was not \
            reported when currencies were queried!"
        ))
    }
}

impl<T> FromIterator<(T, Currency)> for Currencies<T>
where
    T: Borrow<str> + Ord,
{
    fn from_iter<U>(iter: U) -> Self
    where
        U: IntoIterator<Item = (T, Currency)>,
    {
        Self(iter.into_iter().collect())
    }
}

pub struct Currency<T = String>
where
    T: Borrow<str>,
{
    pub dex_symbol: T,
    pub decimal_digits: u8,
}

#[repr(transparent)]
pub struct CurrencyPairs<Dex>(BTreeMap<CurrencyPair, Dex::AssociatedPairData>)
where
    Dex: self::Dex;

impl<Dex> CurrencyPairs<Dex>
where
    Dex: self::Dex,
{
    #[inline]
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (&CurrencyPair, &Dex::AssociatedPairData)> + Send + '_
    {
        self.0.iter()
    }

    #[inline]
    pub fn keys(&self) -> impl Iterator<Item = &CurrencyPair> + Send + '_ {
        self.0.keys()
    }
}

impl<Dex> FromIterator<(CurrencyPair, Dex::AssociatedPairData)>
    for CurrencyPairs<Dex>
where
    Dex: self::Dex,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (CurrencyPair, Dex::AssociatedPairData)>,
    {
        Self(iter.into_iter().collect())
    }
}
