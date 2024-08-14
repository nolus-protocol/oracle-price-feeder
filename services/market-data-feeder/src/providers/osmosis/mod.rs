use prost::Message;
use tonic::codegen::http::uri::PathAndQuery;

mod sealed;

pub struct Osmosis {
    path_and_query: &'static PathAndQuery,
}

#[derive(Clone, Message)]
pub struct SpotPriceRequest {
    #[prost(uint64, tag = "1")]
    pub pool_id: u64,
    #[prost(string, tag = "2")]
    pub base_asset_denom: String,
    #[prost(string, tag = "3")]
    pub quote_asset_denom: String,
}

#[derive(Message)]
pub struct SpotPriceResponse {
    #[prost(string, tag = "1")]
    pub spot_price: String,
}
