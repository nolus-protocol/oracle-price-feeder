use std::{
    borrow::Borrow,
    env,
    num::{
        NonZeroI128, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI8,
        NonZeroU128, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU8,
    },
};

use anyhow::{Context as _, Result};

pub trait ReadFromVar: Sized {
    fn read_from_var<S>(variable: S) -> Result<Self>
    where
        S: Borrow<str> + Into<String>;
}

impl ReadFromVar for String {
    fn read_from_var<S>(variable: S) -> Result<Self>
    where
        S: Borrow<str> + Into<String>,
    {
        let variable = variable.borrow();

        env::var(variable).with_context(|| {
            format!(r#"Failed to read environment variable "{variable}"!"#)
        })
    }
}

macro_rules! impl_for_parseable {
    ($($type: ty),+ $(,)?) => {
        $(
            impl ReadFromVar for $type
            {
                fn read_from_var<S>(
                    variable: S,
                ) -> Result<Self>
                where
                    S: Borrow<str> + Into<String>,
                {
                    String::read_from_var(variable)
                        .and_then(|value| {
                            value.parse()
                                .context(
                                    ::core::concat!(
                                        r#"Failed to parse ""#,
                                        ::core::stringify!($type),
                                        r#""!"#,
                                    ),
                                )
                        })
                }
            }
        )+
    };
}

impl_for_parseable![
    i8,
    NonZeroI8,
    u8,
    NonZeroU8,
    i16,
    NonZeroI16,
    u16,
    NonZeroU16,
    i32,
    NonZeroI32,
    u32,
    NonZeroU32,
    i64,
    NonZeroI64,
    u64,
    NonZeroU64,
    i128,
    NonZeroI128,
    u128,
    NonZeroU128,
];
