use std::{borrow::Borrow, collections::BTreeMap, sync::Arc};

use anyhow::{Context as _, Result};
use serde::Deserialize;

use chain_ops::node;

use self::amount::{Amount, Base, Quote};

pub mod amount;
pub mod providers;

pub trait Dex: Send + Sync + Sized + 'static {
    type ProviderTypeDescriptor;

    type AssociatedPairData: for<'r> Deserialize<'r> + Send + Sync + 'static;

    type PriceQueryMessage: Send + 'static;

    const PROVIDER_TYPE: Self::ProviderTypeDescriptor;

    #[inline]
    fn price_query_messages<Pairs, Ticker>(
        &self,
        pairs: Pairs,
        currencies: &Currencies,
    ) -> Result<BTreeMap<CurrencyPair<Ticker>, Self::PriceQueryMessage>>
    where
        Self: Dex<AssociatedPairData = ()>,
        Pairs: IntoIterator<Item = CurrencyPair<Ticker>>,
        Ticker: Borrow<str> + Ord,
    {
        self.price_query_messages_with_associated_data(
            pairs.into_iter().map(
                #[inline]
                |pair| (pair, ()),
            ),
            currencies,
        )
    }

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
        AssociatedPairData: Borrow<Self::AssociatedPairData>;

    fn price_query(
        &self,
        dex_node_client: &node::Client,
        query_message: &Self::PriceQueryMessage,
    ) -> impl Future<Output = Result<(Amount<Base>, Amount<Quote>)>> + Send + 'static;
}

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurrencyPair<T = Arc<str>>
where
    T: Borrow<str>,
{
    pub base: T,
    pub quote: T,
}
