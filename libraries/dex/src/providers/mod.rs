pub(crate) mod bound_checked;

macro_rules! define_provider {
    ($($module:ident::$provider:ident),+ $(,)?) => {
        use self::{
            $($module::$provider),+
        };

        $(pub mod $module;)+

        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum ProviderType {
            $($provider,)+
        }

        impl ProviderType {
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$provider => ::core::stringify!($provider),)+
                }
            }
        }

        impl ::core::fmt::Display for ProviderType {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.name())
            }
        }

        pub enum Provider {
            $($provider($provider),)+
        }
    };
}

define_provider![astroport::Astroport, osmosis::Osmosis];
