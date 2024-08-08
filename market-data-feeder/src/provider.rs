use std::{collections::BTreeMap, future::Future, sync::Arc};

use anyhow::Result;

use chain_ops::node;

use crate::oracle::Oracle;

pub(crate) trait Provider: Send + Sized {
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
    ) -> impl Future<Output = Result<(BaseAmount, QuoteAmount)>> + Send + 'static;
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecimalAmount {
    amount: String,
    decimal_places: u8,
}

impl DecimalAmount {
    pub const fn new(amount: String, decimal_places: u8) -> Self {
        Self {
            amount,
            decimal_places,
        }
    }

    pub fn into_amount(self) -> String {
        self.amount
    }

    pub const fn decimal_places(&self) -> u8 {
        self.decimal_places
    }
}

macro_rules! define_amount_newtype {
    ($($type:ident),+ $(,)?) => {
        $(
            #[must_use]
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub(crate) struct $type($crate::provider::DecimalAmount);

            impl $type {
                #[inline]
                pub const fn new(value: DecimalAmount) -> Self {
                    Self(value)
                }

                #[inline]
                pub fn into_inner(self) -> DecimalAmount {
                    let Self(value) = self;

                    value
                }
            }
        )+
    };
}

define_amount_newtype![BaseAmount, QuoteAmount];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CurrencyPair {
    pub base: Arc<str>,
    pub quote: Arc<str>,
}
