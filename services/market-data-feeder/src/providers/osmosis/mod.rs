use std::sync::LazyLock;

use prost::Message;
use tonic::codegen::http::uri::PathAndQuery;

#[cfg(test)]
pub(crate) use self::sealed::{greater_than_max_quote_value, MAX_QUOTE_VALUE};

mod sealed;

#[must_use]
pub struct Osmosis {
    path_and_query: &'static PathAndQuery,
}

impl Osmosis {
    pub fn new() -> Self {
        static SINGLETON: LazyLock<PathAndQuery> = LazyLock::new(|| {
            PathAndQuery::from_static(
                "/osmosis.poolmanager.v2.Query/SpotPriceV2",
            )
        });

        Self {
            path_and_query: &SINGLETON,
        }
    }
}

impl Default for Osmosis {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
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
