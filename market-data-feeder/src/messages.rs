use serde::{
    de::{Deserialize, Deserializer},
    Serialize,
};

use crate::price::{Coin, Price};

pub(crate) enum QueryMsg {}

impl QueryMsg {
    pub const CONTRACT_VERSION: &'static [u8] = br#"{"contract_version":{}}"#;

    pub const SUPPORTED_CURRENCY_PAIRS: &'static [u8] =
        br#"{"supported_currency_pairs":{}}"#;

    pub const CURRENCIES: &'static [u8] = br#"{"currencies":{}}"#;
}

pub(crate) type PoolId = u64;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SwapLeg {
    pub from: String,
    pub to: SwapTarget,
}

impl<'de> Deserialize<'de> for SwapLeg {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        <(String, SwapTarget)>::deserialize(deserializer)
            .map(|(from, to): (String, SwapTarget)| Self { from, to })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SwapTarget {
    pub pool_id: PoolId,
    pub target: String,
}

impl<'de> Deserialize<'de> for SwapTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        <(PoolId, String)>::deserialize(deserializer)
            .map(|(pool_id, target): (PoolId, String)| Self { pool_id, target })
    }
}

pub(crate) type SupportedCurrencyPairsResponse = Vec<SwapLeg>;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecuteMsg<C>
where
    C: Coin,
{
    FeedPrices { prices: Box<[Price<C>]> },
}
