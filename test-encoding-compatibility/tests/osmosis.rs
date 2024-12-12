use osmosis_std::types::osmosis::poolmanager::v2::{
    SpotPriceRequest as LibraryRequest, SpotPriceResponse as LibraryResponse,
};
use prost::Message as _;

use market_data_feeder::providers::osmosis::{
    SpotPriceRequest as LocalRequest, SpotPriceResponse as LocalResponse,
};

#[test]
fn request() {
    const POOL_ID: u64 = 0x123;

    const BASE_ASSET: &str = "456";

    const QUOTE_ASSET: &str = "789";

    let local = LocalRequest {
        pool_id: POOL_ID,
        base_asset_denom: BASE_ASSET.to_string(),
        quote_asset_denom: QUOTE_ASSET.to_string(),
    };

    let mut protobuf = vec![];

    () = local.encode(&mut protobuf).unwrap();

    let library = LibraryRequest::decode(&*protobuf).unwrap();

    assert_eq!(library.pool_id, POOL_ID);

    assert_eq!(&*library.base_asset_denom, BASE_ASSET);

    assert_eq!(&*library.quote_asset_denom, QUOTE_ASSET);
}

#[test]
fn response() {
    const SPOT_PRICE: &str = "123456789";

    let library = LibraryResponse {
        spot_price: SPOT_PRICE.to_string(),
    };

    let mut protobuf = vec![];

    () = library.encode(&mut protobuf).unwrap();

    let local = LocalResponse::decode(&*protobuf).unwrap();

    assert_eq!(&*local.spot_price, SPOT_PRICE);
}
