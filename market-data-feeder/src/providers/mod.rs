use crate::provider::{ComparisonProvider, FromConfig, Provider};

use self::osmosis::Osmosis;

mod osmosis;

pub(crate) struct Providers;

impl Providers {
    pub fn visit_provider<V>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ProviderVisitor,
    {
        match id {
            Osmosis::ID => Some(visitor.on::<Osmosis>()),
            _ => None,
        }
    }

    pub fn visit_comparison_provider<V>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ComparisonProviderVisitor,
    {
        match id {
            Osmosis::ID => Some(visitor.on::<Osmosis>()),
            _ => None,
        }
    }
}

pub(crate) trait ProviderVisitor {
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: Provider + FromConfig;
}

pub(crate) trait ComparisonProviderVisitor {
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: ComparisonProvider + FromConfig;
}
