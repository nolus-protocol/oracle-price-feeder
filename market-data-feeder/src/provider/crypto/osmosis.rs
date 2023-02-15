use std::{borrow::Cow, collections::BTreeMap};

use async_trait::async_trait;
use reqwest::{Client as ReqwestClient, RequestBuilder, StatusCode, Url};
use serde::{Deserialize, Deserializer};
use tracing::error;

use chain_comms::{client::Client as NodeClient, interact::query_wasm};

use crate::{
    config::{Symbol, Ticker},
    messages::{QueryMsg, SupportedCurrencyPairsResponse},
    provider::{FeedProviderError, Price, Provider},
};

#[derive(Debug, Deserialize)]
struct AssetPrice {
    spot_price: Ratio,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Ratio {
    numerator: u128,
    denominator: u128,
}

impl<'de> Deserialize<'de> for Ratio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let point;

        let spot_price = {
            let mut spot_price = String::deserialize(deserializer)?;

            point = if let Some(point) = spot_price.find('.') {
                spot_price = spot_price.trim_end_matches('0').into();

                spot_price.remove(point);

                point
            } else {
                spot_price.len()
            };

            spot_price
        };

        Ok(Ratio {
            numerator: spot_price
                .trim_start_matches('0')
                .parse()
                .map_err(serde::de::Error::custom)?,
            denominator: 10_u128
                .checked_pow(
                    (spot_price.len() - point)
                        .try_into()
                        .map_err(serde::de::Error::custom)?,
                )
                .ok_or_else(|| {
                    serde::de::Error::custom("Couldn't calculate ratio! Exponent too big!")
                })?,
        })
    }
}

pub struct Client {
    http_client: ReqwestClient,
    base_url: Url,
    currencies: BTreeMap<Ticker, Symbol>,
    supported_currencies_query: Vec<u8>,
}

impl Client {
    pub fn new(
        url_str: &str,
        currencies: &BTreeMap<Ticker, Symbol>,
    ) -> Result<Self, FeedProviderError> {
        match Url::parse(url_str) {
            Ok(base_url) => Ok(Self {
                http_client: ReqwestClient::new(),
                base_url,
                currencies: currencies.clone(),
                supported_currencies_query: serde_json_wasm::to_vec(
                    &QueryMsg::SupportedCurrencyPairs {},
                )?,
            }),
            Err(err) => {
                eprintln!("{err:?}");

                Err(FeedProviderError::InvalidProviderURL(url_str.to_string()))
            }
        }
    }

    fn get_request_builder(&self, url_str: &str) -> Result<RequestBuilder, FeedProviderError> {
        self.base_url
            .join(url_str)
            .map(|url| self.http_client.get(url))
            .map_err(|_| FeedProviderError::URLParsingError)
    }
}

#[async_trait]
impl Provider for Client {
    fn name(&self) -> Cow<'static, str> {
        "Osmosis".into()
    }

    async fn get_spot_prices(
        &self,
        node_client: &NodeClient,
        oracle_addr: &str,
    ) -> Result<Box<[Price]>, FeedProviderError> {
        let mut prices = vec![];

        for (pool_id, (from_ticker, from_symbol), (to_ticker, to_symbol)) in
            query_wasm::<SupportedCurrencyPairsResponse>(
                node_client,
                oracle_addr,
                &self.supported_currencies_query,
            )
            .await?
            .into_iter()
            .filter_map(|swap| {
                let from_symbol = self.currencies.get(&swap.from).cloned()?;
                let to_symbol = self.currencies.get(&swap.to.target).cloned()?;

                Some((
                    swap.to.pool_id,
                    (swap.from, from_symbol),
                    (swap.to.target, to_symbol),
                ))
            })
        {
            let resp = self
                .get_request_builder(&format!("pools/{pool_id}/prices"))
                .unwrap()
                .query(&[
                    ("base_asset_denom", from_symbol),
                    ("quote_asset_denom", to_symbol),
                ])
                .send()
                .await
                .map_err(FeedProviderError::FetchPoolPrice)?;

            if resp.status() == StatusCode::OK {
                let AssetPrice {
                    spot_price:
                        Ratio {
                            numerator: base,
                            denominator: quote,
                        },
                } = resp
                    .json()
                    .await
                    .map_err(FeedProviderError::DeserializePoolPrice)?;

                prices.push(Price::new(from_ticker, base, to_ticker, quote));
            } else {
                error!(
                    from = %from_ticker,
                    to = %to_ticker,
                    "Couldn't resolve spot price! Server returned status code {}!",
                    resp.status().as_u16()
                );
            }
        }

        Ok(prices.into_boxed_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::Ratio;

    #[test]
    fn deserialize_ratio_gt1() {
        use serde_json_wasm::from_str;

        assert_eq!(
            from_str::<Ratio>("\"1.234\"").unwrap(),
            Ratio {
                numerator: 1234,
                denominator: 1000,
            }
        );
    }

    #[test]
    fn deserialize_ratio_lt1() {
        use serde_json_wasm::from_str;

        assert_eq!(
            from_str::<Ratio>("\"0.1234\"").unwrap(),
            Ratio {
                numerator: 1234,
                denominator: 10000,
            }
        );
    }

    #[test]
    fn deserialize_ratio_eq2() {
        use serde_json_wasm::from_str;

        assert_eq!(
            from_str::<Ratio>("\"2\"").unwrap(),
            Ratio {
                numerator: 2,
                denominator: 1,
            }
        );
    }

    #[test]
    fn deserialize_ratio_eq16k() {
        use serde_json_wasm::from_str;

        assert_eq!(
            from_str::<Ratio>("\"16000.000000000000001\"").unwrap(),
            Ratio {
                numerator: 16000000000000000001,
                denominator: 1000000000000000,
            }
        );
    }
}
