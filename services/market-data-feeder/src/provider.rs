use std::{
    collections::BTreeMap, fmt::Debug, future::Future, marker::PhantomData,
    sync::Arc,
};

use anyhow::Result;

use chain_ops::node;

use crate::oracle::Oracle;

pub trait Provider: Send + Sized {
    type PriceQueryMessage: Send + 'static;

    const PROVIDER_NAME: &'static str;

    fn price_query_messages(
        &self,
        oracle: &Oracle,
    ) -> Result<BTreeMap<CurrencyPair, Self::PriceQueryMessage>>;

    fn price_query(
        &self,
        dex_node_client: &node::Client,
        query_message: &Self::PriceQueryMessage,
    ) -> impl Future<Output = Result<(Amount<Base>, Amount<Quote>)>> + Send + 'static;
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal {
    amount: String,
    decimal_places: u8,
}

impl Decimal {
    #[inline]
    pub const fn new(amount: String, decimal_places: u8) -> Self {
        Self {
            amount,
            decimal_places,
        }
    }

    #[inline]
    pub fn amount(&self) -> &str {
        &self.amount
    }

    #[inline]
    pub fn into_amount(self) -> String {
        self.amount
    }

    #[inline]
    pub const fn decimal_places(&self) -> u8 {
        self.decimal_places
    }
}

pub trait Marker: Debug + Copy + Eq {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {}

impl Marker for Base {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {}

impl Marker for Quote {}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amount<T: Marker> {
    amount: Decimal,
    _marker: PhantomData<T>,
}

impl<T> Amount<T>
where
    T: Marker,
{
    #[inline]
    pub const fn new(amount: Decimal) -> Self {
        Self {
            amount,
            _marker: const { PhantomData },
        }
    }

    #[inline]
    pub const fn as_inner(&self) -> &Decimal {
        &self.amount
    }

    #[inline]
    pub fn into_inner(self) -> Decimal {
        self.amount
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurrencyPair {
    pub base: Arc<str>,
    pub quote: Arc<str>,
}
