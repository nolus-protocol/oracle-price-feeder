use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

use crate::config::{Symbol, Ticker};

pub(in super::super) fn deserialize<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<Ticker, Symbol>, D::Error>
where
    D: Deserializer<'de>,
{
    <Vec<(Ticker, Symbol)> as Deserialize>::deserialize(deserializer)
        .map(|currencies| currencies.into_iter().collect())
}
