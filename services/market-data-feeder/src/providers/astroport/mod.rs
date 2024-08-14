use serde::{Deserialize, Serialize};

mod sealed;

pub struct Astroport {
    router_address: String,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum QueryMsg {
    SimulateSwapOperations {
        offer_amount: String,
        operations: [SwapOperation; 1],
    },
}

#[derive(Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum SwapOperation {
    AstroSwap {
        offer_asset_info: AssetInfo,
        ask_asset_info: AssetInfo,
    },
}

#[derive(Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum AssetInfo {
    NativeToken { denom: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SimulateSwapOperationsResponse {
    pub amount: String,
}
