use std::{collections::BTreeMap, convert::identity};

use tokio::task::JoinSet;

use crate::{
    price::{Coin, Price},
    provider::{ComparisonProvider, FromConfig, Provider, ProviderError},
};

use self::{
    astroport::Astroport, coin_gecko::SanityCheck as CoinGeckoSanityCheck, osmosis::Osmosis,
};

mod astroport;
mod coin_gecko;
mod osmosis;

pub(crate) struct Providers;

impl Providers {
    pub fn visit_provider<V>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ProviderVisitor,
    {
        match id {
            <Astroport as FromConfig<false>>::ID => Some(visitor.on::<Astroport>()),
            <Osmosis as FromConfig<false>>::ID => Some(visitor.on::<Osmosis>()),
            _ => None,
        }
    }

    pub fn visit_comparison_provider<V>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ComparisonProviderVisitor,
    {
        match id {
            CoinGeckoSanityCheck::ID => Some(visitor.on::<CoinGeckoSanityCheck>()),
            _ => Self::visit_provider(id, ProviderConversionVisitor(visitor)),
        }
    }
}

struct ProviderConversionVisitor<V: ComparisonProviderVisitor>(V);

impl<V: ComparisonProviderVisitor> ProviderVisitor for ProviderConversionVisitor<V> {
    type Return = V::Return;

    fn on<P>(self) -> Self::Return
    where
        P: Provider + FromConfig<false>,
    {
        self.0.on::<P>()
    }
}

pub(crate) trait ProviderVisitor {
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: Provider + FromConfig<false>;
}

pub(crate) trait ComparisonProviderVisitor {
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: ComparisonProvider + FromConfig<true>;
}

fn left_over_fields(config: BTreeMap<String, toml::Value>) -> Option<Box<str>> {
    config
        .into_keys()
        .reduce(|mut accumulator: String, key: String| {
            accumulator.reserve(key.len() + 2);

            accumulator.push_str(", ");

            accumulator.push_str(&key);

            accumulator
        })
        .map(String::into_boxed_str)
}

async fn collect_prices_from_task_set<C>(
    mut set: JoinSet<Result<Price<C>, ProviderError>>,
    fault_tolerant: bool,
) -> Result<Box<[Price<C>]>, ProviderError>
where
    C: Coin,
{
    let mut prices: Vec<Price<C>> = Vec::with_capacity(set.len());

    while let Some(result) = set.join_next().await {
        match result.map_err(From::from).and_then(identity) {
            Ok(price) => prices.push(price),
            Err(error) if fault_tolerant => {
                tracing::error!(error = %error, "Couldn't resolve price!")
            }
            Err(error) => return Err(error),
        }
    }

    Ok(prices.into_boxed_slice())
}
