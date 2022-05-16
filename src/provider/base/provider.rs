use std::{collections::HashSet, str::FromStr};

use async_trait::async_trait;
use cosmwasm_std::Decimal256;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::{
    configuration,
    cosm_client::CosmClient,
    provider::{CryptoProviderType, CryptoProvidersFactory},
};

#[derive(Error, Debug)]
pub enum FeedProviderError {
    #[error("Asset pair not found in pool")]
    AssetPairNotFound,

    #[error("Denom {denom} not found")]
    DenomNotFound { denom: String },

    #[error("Invalid poll. Empty weight")]
    InvalidPoolEmptyWeight,

    #[error("No price found for pair [ {base} / {quote} ]")]
    NoPriceFound { base: String, quote: String },

    #[error("Request error. Cause: {message}")]
    RequestError { message: String },

    #[error("Invalid provider url {0}")]
    InvalidProviderURL(String),

    #[error("URL parsing error")]
    URLParsingError,

    #[error("Unexpected error")]
    UnexpectedError,

    #[error("Unsupported provider type {0}")]
    UnsupportedProviderType(String),

    #[error("{0}")]
    ReqwestError(#[from] reqwest::Error),
}

#[async_trait]
pub trait Provider {
    async fn get_spot_price(
        &self,
        base_denom: &str,
        quote_denom: &str,
    ) -> Result<Decimal256, FeedProviderError>;
    async fn single_run(
        &self,
        denoms: &[Vec<String>],
        cosm_client: &CosmClient,
    ) -> Result<(), FeedProviderError>;

    async fn continuous(
        &self,
        denoms: &[Vec<String>],
        cosm_client: &CosmClient,
        tick_time: u64,
    ) -> Result<(), FeedProviderError>;
}

#[derive(Debug, PartialEq)]
pub enum ProviderType {
    Crypto,
}

impl FromStr for ProviderType {
    type Err = ();
    fn from_str(input: &str) -> Result<ProviderType, Self::Err> {
        match input {
            "crypto" => Ok(ProviderType::Crypto),
            _ => Err(()),
        }
    }
}

pub struct ProvidersFactory;
impl ProvidersFactory {
    pub fn new_provider(
        s: &ProviderType,
        cfg: &configuration::Providers,
    ) -> Result<Box<dyn Provider>, FeedProviderError> {
        match s {
            ProviderType::Crypto => {
                let provider_type = match CryptoProviderType::from_str(&cfg.name) {
                    Ok(t) => t,
                    Err(_) => {
                        return Err(FeedProviderError::UnsupportedProviderType(
                            cfg.name.to_owned(),
                        ))
                    }
                };
                CryptoProvidersFactory::new_provider(&provider_type, &cfg.base_address)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DenomPairPrice {
    pub base: String,
    pub quote: String,
    pub price: Decimal256,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BaseDenomPrices {
    pub base: String,
    pub prices: Vec<(String, Decimal256)>,
}

pub async fn push_prices(prices: &[DenomPairPrice], cosm_client: &CosmClient) {
    let unique_bases: HashSet<String> = prices
        .iter()
        .map(|price| price.base.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut all_prices = vec![];

    for base in unique_bases {
        let base_prices = prices
            .iter()
            .filter(|el| el.base.eq(&base))
            .map(|el| (el.quote.clone(), el.price))
            .collect::<Vec<(String, Decimal256)>>();

        all_prices.push(BaseDenomPrices {
            base,
            prices: base_prices,
        });
    }

    if all_prices.is_empty() {
        println!("No prices found to push");
        return;
    }

    let price_feed_json = json!({
        "feed_prices": {
            "prices": all_prices
        }
    });

    println!("JSON request: ");
    println!("{}", price_feed_json);

    let res = cosm_client
        .generate_and_send_tx(&price_feed_json.to_string())
        .await;

    if res.is_err() {
        println!("{:?}", res.unwrap_err());
    }
}

pub async fn get_supported_denom_pairs(
    cosm_client: &CosmClient,
) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    let query_json = json!({
        "supported_denom_pairs": {}
    });

    let resp = cosm_client.query_message(&query_json.to_string()).await?;

    Ok(serde_json::from_slice(&resp.data)?)
}

#[cfg(test)]
mod tests {
    use crate::{
        configuration::Providers,
        provider::{ProviderType, ProvidersFactory},
    };

    use std::str::FromStr;
    const TEST_OSMOSIS_URL: &str = "https://lcd-osmosis.keplr.app/osmosis/gamm/v1beta1/";

    #[test]
    fn get_provider() {
        let t = ProviderType::from_str("crypto").unwrap();
        assert_eq!(ProviderType::Crypto, t);
        ProviderType::from_str("invalid").unwrap_err();
        ProvidersFactory::new_provider(
            &ProviderType::Crypto,
            &Providers {
                main_type: "crypto".to_string(),
                name: "osmosis".to_string(),
                base_address: TEST_OSMOSIS_URL.to_string(),
            },
        )
        .unwrap();
    }
}
