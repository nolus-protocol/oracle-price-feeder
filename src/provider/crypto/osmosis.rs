use std::{time::Duration, vec};

use super::osmosis_pool::Pool;
use crate::{
    cosm_client::CosmClient,
    provider::{push_prices, DenomPairPrice, FeedProviderError, Provider},
};
use async_trait::async_trait;
use cosmwasm_std::Decimal256;
use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;
use tokio::time;

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
        match reqwest::Url::parse(url_str) {
            Ok(base_url) => Ok(Self { base_url }),
            Err(err) => {
                eprintln!("{:?}", err);
                Err(FeedProviderError::InvalidProviderURL(url_str.to_string()))
            }
        }
    }

    fn get_request_bulder(&self, url_str: &str) -> Result<RequestBuilder, FeedProviderError> {
        let http_client = Client::new();
        match self.base_url.join(url_str) {
            Ok(url) => Ok(http_client.get(url)),
            Err(_) => Err(FeedProviderError::URLParsingError),
        }
    }

    pub async fn get_pools(&self, limit: usize) -> Result<Vec<Pool>, FeedProviderError> {
        let resp = self
            .get_request_bulder("pools")?
            .query(&[("pagination.limit", limit)])
            .send()
            .await?;

        Ok(resp.json::<OsmosisResponse>().await?.pools)
    }

    pub async fn get_pools_count(&self) -> Result<usize, FeedProviderError> {
        let resp = self.get_request_bulder("num_pools")?.send().await?;
        let parsed = resp.json::<PoolsCountResponse>().await?;
        Ok(parsed.num_pools.parse::<usize>().unwrap_or_default())
    }

    fn walk_pools(
        pools: &[Pool],
        base_denom: &str,
        quote_denom: &str,
    ) -> Result<Decimal256, FeedProviderError> {
        for pool in pools {
            let res = pool.spot_price(base_denom, quote_denom);
            if let Ok(..) = res {
                let price = res.unwrap();
                println!(
                    "Assets pair found in pool with id {} price {}",
                    pool.id, price
                );
                return Ok(price);
            }
        }

        Err(FeedProviderError::NoPriceFound {
            base: base_denom.to_string(),
            quote: quote_denom.to_string(),
        })
    }
}

#[async_trait]
impl Provider for OsmosisClient {
    async fn get_spot_price(
        &self,
        base_denom: &str,
        quote_denom: &str,
    ) -> Result<Decimal256, FeedProviderError> {
        let resp = self.get_pools_count().await;
        let all_pools_resp = match resp {
            Ok(cnt) => self.get_pools(cnt).await,
            Err(err) => {
                return Err(FeedProviderError::RequestError {
                    message: err.to_string(),
                });
            }
        };

        let res = match all_pools_resp {
            Ok(pools) => OsmosisClient::walk_pools(&pools, base_denom, quote_denom),
            Err(err) => {
                return Err(FeedProviderError::RequestError {
                    message: err.to_string(),
                });
            }
        };

        match res {
            Ok(price) => return Ok(price),
            Err(err) => return Err(err),
        }
    }

    async fn single_run(
        &self,
        denoms: &[Vec<String>],
        cosm_client: &CosmClient,
    ) -> Result<(), FeedProviderError> {
        let resp = self.get_pools_count().await;
        let all_pools_resp = match resp {
            Ok(cnt) => self.get_pools(cnt).await,
            Err(err) => return Err(err),
        };

        let mut prices: Vec<DenomPairPrice> = vec![];
        match all_pools_resp {
            Ok(pools) => {
                for denom_pair in denoms {
                    let base_denom = denom_pair
                        .get(0)
                        .ok_or(FeedProviderError::AssetPairNotFound)?;
                    let quote_denom = denom_pair
                        .get(1)
                        .ok_or(FeedProviderError::AssetPairNotFound)?;
                    println!("Checking denom pair {} / {}", base_denom, quote_denom);

                    let price = OsmosisClient::walk_pools(&pools, base_denom, quote_denom)
                        .unwrap_or_default();
                    if !price.is_zero() {
                        prices.push(DenomPairPrice {
                            base: base_denom.to_string(),
                            quote: quote_denom.to_string(),
                            price,
                        })
                    } else {
                        println!(
                            "No price found for denom pair {} / {}",
                            base_denom, quote_denom
                        );
                    }
                }
            }
            Err(err) => return Err(err),
        };

        println!("Prices: ");
        println!("{:?}", prices);

        push_prices(&prices, cosm_client).await;

        Ok(())
    }

    async fn continuous(
        &self,
        denoms: &[Vec<String>],
        cosm_client: &CosmClient,
        tick_time: u64,
    ) -> Result<(), FeedProviderError> {
        let mut interval = time::interval(Duration::from_secs(tick_time));

        for _i in 0.. {
            interval.tick().await;
            self.single_run(denoms, cosm_client).await?;
        }

        Err(FeedProviderError::UnexpectedError)
    }
}
