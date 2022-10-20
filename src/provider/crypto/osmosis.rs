use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder, StatusCode, Url};
use serde::{de::Unexpected, Deserialize, Deserializer};

use crate::{
    cosmos::Client as CosmosClient,
    provider::{get_supported_denom_pairs, FeedProviderError, Price, Provider},
};

#[derive(Debug, Deserialize)]
struct AssetPrice {
    #[serde(deserialize_with = "deserialize_spot_price")]
    spot_price: Ratio,
}

#[derive(Debug)]
struct Ratio {
    numerator: u128,
    denominator: u128,
}

fn deserialize_spot_price<'de, D>(deserializer: D) -> Result<Ratio, D::Error>
where
    D: Deserializer<'de>,
{
    let point;

    let spot_price = {
        let mut spot_price = String::deserialize(deserializer)?;

        point = spot_price.find('.').ok_or_else(|| {
            serde::de::Error::invalid_value(
                Unexpected::Str(&spot_price),
                &"Expected decimal value with point separator!",
            )
        })?;

        spot_price.remove(point);

        spot_price
    };

    Ok(Ratio {
        numerator: 10_u128.pow(
            (spot_price.len() - point)
                .try_into()
                .map_err(serde::de::Error::custom)?,
        ),
        denominator: spot_price
            .trim_start_matches('0')
            .parse()
            .map_err(serde::de::Error::custom)?,
    })
}

pub struct Client {
    base_url: Url,
}

impl Client {
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
        let http_client = ReqwestClient::new();

        self.base_url
            .join(url_str)
            .map(|url| http_client.get(url))
            .map_err(|_| FeedProviderError::URLParsingError)
    }
}

#[async_trait]
impl Provider for Client {
    async fn get_spot_prices(
        &self,
        cosm_client: &CosmosClient,
    ) -> Result<Vec<Price>, FeedProviderError> {
        let mut prices = vec![];

        for pair in get_supported_denom_pairs(cosm_client).await? {
            let resp = self
                .get_request_builder(&format!("pools/{id}/prices", id = pair.to.pool_id))
                .unwrap()
                .query(&[
                    ("base_asset_denom", pair.from.as_str()),
                    ("quote_asset_denom", pair.to.target.as_str()),
                ])
                .send()
                .await?;

            assert_eq!(resp.status(), StatusCode::OK);

            let AssetPrice {
                spot_price:
                    Ratio {
                        numerator: base,
                        denominator: quote,
                    },
            } = resp.json().await?;

            prices.push(Price::new(pair.from, base, pair.to.target, quote));
        }

        Ok(prices)
    }
}
