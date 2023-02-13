use serde::{Deserialize, Serialize};

use crate::provider::Price;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    ContractVersion {},
    SupportedCurrencyPairs {},
}

pub type PoolId = u64;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SwapLeg {
    pub from: String,
    pub to: SwapTarget,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SwapTarget {
    pub pool_id: PoolId,
    pub target: String,
}

pub type SupportedCurrencyPairsResponse = Vec<SwapLeg>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    FeedPrices { prices: Box<[Price]> },
}
