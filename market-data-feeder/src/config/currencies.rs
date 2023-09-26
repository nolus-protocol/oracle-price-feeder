use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

use super::{Symbol, Ticker};

#[derive(Deserialize)]
#[must_use]
#[repr(transparent)]
#[serde(transparent)]
pub(crate) struct Currencies(
    #[serde(deserialize_with = "self::deserialize")] pub BTreeMap<Ticker, Symbol>,
);

fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<Ticker, Symbol>, D::Error>
where
    D: Deserializer<'de>,
{
    <Vec<(Ticker, Symbol)> as Deserialize>::deserialize(deserializer)
        .map(|currencies| currencies.into_iter().collect())
}
