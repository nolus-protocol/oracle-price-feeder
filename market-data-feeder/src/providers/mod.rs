use crate::{
    config::ProviderConfigExt,
    provider::{ComparisonProvider, ProviderSized},
};

use self::osmosis::Osmosis;

mod osmosis;

pub(crate) struct Providers;

impl Providers {
    pub fn visit_provider<V, Config>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ProviderVisitor<Config>,
        Config: ProviderConfigExt,
    {
        match id {
            <Osmosis as ProviderSized<Config>>::ID => Some(visitor.on::<Osmosis>()),
            _ => None,
        }
    }

    pub fn visit_comparison_provider<V, Config>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: ComparisonProviderVisitor<Config>,
        Config: ProviderConfigExt,
    {
        match id {
            <Osmosis as ProviderSized<Config>>::ID => Some(visitor.on::<Osmosis>()),
            _ => None,
        }
    }
}

pub(crate) trait ProviderVisitor<Config>
where
    Config: ProviderConfigExt,
{
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: ProviderSized<Config>;
}

pub(crate) trait ComparisonProviderVisitor<Config>
where
    Config: ProviderConfigExt,
{
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: ProviderSized<Config> + ComparisonProvider;
}
