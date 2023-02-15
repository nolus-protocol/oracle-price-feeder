use serde::{Deserialize, Deserializer, Serialize};

use crate::provider::Price;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ContractVersion {},
    SupportedCurrencyPairs {},
}

pub type PoolId = u64;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SwapLeg {
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
pub struct SwapTarget {
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

pub type SupportedCurrencyPairsResponse = Vec<SwapLeg>;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    FeedPrices { prices: Box<[Price]> },
}
