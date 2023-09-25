use crate::{config::ProviderExtTrait, provider::ProviderSized};

use self::osmosis::Osmosis;

mod osmosis;

pub(crate) struct Providers;

impl Providers {
    pub fn visit<V, Config>(id: &str, visitor: V) -> Option<V::Return>
    where
        V: Visitor<Config>,
        Config: ProviderExtTrait,
    {
        match id {
            <Osmosis as ProviderSized<Config>>::ID => Some(visitor.on::<Osmosis>()),
            _ => None,
        }
    }
}

pub(crate) trait Visitor<Config>
where
    Config: ProviderExtTrait,
{
    type Return;

    fn on<P>(self) -> Self::Return
    where
        P: ProviderSized<Config>;
}
