use crate::provider::{ComparisonProvider, FromConfig, Provider};

use self::{coin_gecko::SanityCheck as CoinGeckoSanityCheck, osmosis::Osmosis};

mod coin_gecko;
mod osmosis;

pub(crate) struct Providers;

impl Providers {
    pub fn visit_provider<V>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ProviderVisitor,
    {
        match id {
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
            _ => Self::visit_provider(id, VW(visitor)),
        }
    }
}

struct VW<V: ComparisonProviderVisitor>(V);

impl<V: ComparisonProviderVisitor> ProviderVisitor for VW<V> {
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
