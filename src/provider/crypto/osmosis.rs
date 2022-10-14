use std::vec;

use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;

use crate::provider::{FeedProviderError, Price, Provider};

use super::osmosis_pool::Pool;

#[derive(Deserialize, Debug)]
pub struct OsmosisResponse {
    pools: Vec<Pool>,
}

#[derive(Deserialize, Debug)]
pub struct PoolsCountResponse {
    #[serde(rename = "numPools")]
    num_pools: String,
}

pub struct OsmosisClient {
    base_url: Url,
}

impl OsmosisClient {
    pub fn new(url_str: &str) -> Result<Self, FeedProviderError> {
        match Url::parse(url_str) {
            Ok(base_url) => Ok(Self { base_url }),
            Err(err) => {
                eprintln!("{:?}", err);

                Err(FeedProviderError::InvalidProviderURL(url_str.to_string()))
            }
        }
    }

    fn get_request_builder(&self, url_str: &str) -> Result<RequestBuilder, FeedProviderError> {
        let http_client = Client::new();

        self.base_url.join(url_str).map(|url| http_client.get(url)).map_err(|_| FeedProviderError::URLParsingError)
    }

    pub async fn get_pools(&self, limit: usize) -> Result<Vec<Pool>, FeedProviderError> {
        let resp = self
            .get_request_builder("pools")?
            .query(&[("pagination.limit", limit)])
            .send()
            .await?;

        Ok(resp.json::<OsmosisResponse>().await?.pools)
    }

    pub async fn get_pools_count(&self) -> Result<usize, FeedProviderError> {
        let resp = self.get_request_builder("num_pools")?.send().await?;

        let parsed = resp.json::<PoolsCountResponse>().await?;

        Ok(parsed.num_pools.parse::<usize>().unwrap_or_default())
    }

    fn walk_pools(
        pools: &[Pool],
        base_denom: &str,
        quote_denom: &str,
    ) -> Result<Price, FeedProviderError> {
        for pool in pools {
            let res = pool.spot_price(base_denom, quote_denom);

            if let Ok(price) = res {
                println!(
                    "Assets pair found in pool with id {} price {:?}",
                    pool.id, price
                );

                return Ok(price);
            }
        }

        Err(FeedProviderError::NoPriceFound {
            base: String::from(base_denom),
            quote: String::from(quote_denom),
        })
    }
}

#[async_trait]
impl Provider for OsmosisClient {
    async fn get_spot_prices(
        &self,
        denoms: &[Vec<String>],
        // cosm_client: &CosmosClient,
    ) -> Result<Vec<Price>, FeedProviderError> {
        let pools = self.get_pools(self.get_pools_count().await?).await?;

        let mut prices: Vec<Price> = vec![];

        for denom_pair in denoms {
            let base_denom = denom_pair
                .first()
                .ok_or(FeedProviderError::AssetPairNotFound)?;

            let quote_denom = denom_pair
                .get(1)
                .ok_or(FeedProviderError::AssetPairNotFound)?;

            println!("Checking denom pair {} / {}", base_denom, quote_denom);

            if let Ok(price) = OsmosisClient::walk_pools(&pools, base_denom, quote_denom) {
                prices.push(price)
            } else {
                println!(
                    "No price found for denom pair {} / {}",
                    base_denom, quote_denom
                );
            }
        }

        println!("Prices: {:#?}", prices);

        // push_prices(&prices, cosm_client).await;

        Ok(prices)
    }
}
