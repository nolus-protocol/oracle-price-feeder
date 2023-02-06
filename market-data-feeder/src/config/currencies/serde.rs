use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

use crate::config::{Symbol, Ticker};

pub(in super::super) fn serialize<S>(
    currencies: &BTreeMap<Ticker, Symbol>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    currencies
        .iter()
        .collect::<Vec<(&Ticker, &Symbol)>>()
        .serialize(serializer)
}

pub(in super::super) fn deserialize<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<Ticker, Symbol>, D::Error>
where
    D: Deserializer<'de>,
{
    <Vec<(Ticker, Symbol)> as Deserialize>::deserialize(deserializer)
        .map(|currencies| currencies.into_iter().collect())
}
