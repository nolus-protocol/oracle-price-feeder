use std::str::FromStr;

use async_trait::async_trait;

use crate::{
    configuration::{self},
    cosmos::{CosmosClient, QueryMsg},
    provider::{CryptoProviderType, CryptoProvidersFactory},
};

use super::{FeedProviderError, Price};

#[async_trait]
pub trait Provider {
    async fn get_spot_prices(
        &self,
        denoms: &[Vec<String>],
    ) -> Result<Vec<Price>, FeedProviderError>;
}

#[derive(Debug, PartialEq, Eq)]
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
                let provider_type = CryptoProviderType::from_str(&cfg.name)
                    .map_err(|_| FeedProviderError::UnsupportedProviderType(cfg.name.clone()))?;

                CryptoProvidersFactory::new_provider(&provider_type, &cfg.base_address)
            }
        }
    }
}

pub async fn get_supported_denom_pairs(
    cosm_client: &CosmosClient,
) -> Result<Vec<Vec<String>>, FeedProviderError> {
    cosm_client
        .cosmwasm_query(&QueryMsg::SupportedDenomPairs {})
        .await
        .map_err(Into::into)
        .and_then(|resp| serde_json::from_slice(&resp.data).map_err(Into::into))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        configuration::Providers,
        provider::{ProviderType, ProvidersFactory},
    };

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
