use std::{borrow::Borrow, env, num::NonZero};

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
            impl_for_parseable!(@@@ $type);
            impl_for_parseable!(@@@ NonZero<$type>);
        )+
    };
    (@@@ $type:ty) => {
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
    };
}

impl_for_parseable![i8, u8, i16, u16, i32, u32, i64, u64, i128, u128];
