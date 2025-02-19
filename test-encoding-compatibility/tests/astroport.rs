use astroport::{
    asset::AssetInfo as LibraryAssetInfo,
    router::{
        QueryMsg as LibraryQueryMsg,
        SimulateSwapOperationsResponse as LibraryResponse,
        SwapOperation as LibrarySwapOperation,
    },
};
use serde_json_wasm::{from_str, to_string};

use dex::providers::astroport::{
    AssetInfo as LocalAssetInfo, QueryMsg as LocalQueryMsg,
    SimulateSwapOperationsResponse as LocalResponse,
    SwapOperation as LocalSwapOperation,
};

#[test]
fn request() {
    const OFFER_AMOUNT: u128 = 123;

    const OFFER_ASSET: &str = "456";

    const ASK_ASSET: &str = "789";

    let local = LocalQueryMsg::SimulateSwapOperations {
        offer_amount: OFFER_AMOUNT.to_string(),
        operations: [LocalSwapOperation::AstroSwap {
            offer_asset_info: LocalAssetInfo::NativeToken {
                denom: OFFER_ASSET.to_string(),
            },
            ask_asset_info: LocalAssetInfo::NativeToken {
                denom: ASK_ASSET.to_string(),
            },
        }],
    };

    let json = to_string(&local).unwrap();

    let LibraryQueryMsg::SimulateSwapOperations {
        offer_amount,
        operations,
    } = from_str(&json).unwrap()
    else {
        panic!()
    };

    let [LibrarySwapOperation::AstroSwap {
        offer_asset_info: LibraryAssetInfo::NativeToken { denom: offer_asset },
        ask_asset_info: LibraryAssetInfo::NativeToken { denom: ask_asset },
    }] = &*operations
    else {
        panic!()
    };

    assert_eq!(offer_amount.u128(), OFFER_AMOUNT);

    assert_eq!(offer_asset, OFFER_ASSET);

    assert_eq!(ask_asset, ASK_ASSET);
}

#[test]
fn response() {
    const SPOT_PRICE: u128 = 1234;

    let library = LibraryResponse {
        amount: SPOT_PRICE.into(),
    };

    let json = to_string(&library).unwrap();

    let local: LocalResponse = from_str(&json).unwrap();

    let x: u128 = local.amount.parse().unwrap();

    assert_eq!(x, SPOT_PRICE);
}
