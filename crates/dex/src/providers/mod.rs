pub(crate) mod bound_checked;

macro_rules! define_provider {
    ($($module:ident::$provider:ident),+ $(,)?) => {
        use self::{
            $($module::$provider),+
        };

        $(pub mod $module;)+

        pub enum Provider {
            $($provider($provider),)+
        }
    };
}

define_provider![astroport::Astroport, osmosis::Osmosis];
