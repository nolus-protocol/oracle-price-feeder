use std::str::FromStr;

use async_trait::async_trait;

use crate::{
    configuration,
    cosmos::{Client, QueryMsg, SupportedDenomPairsResponse},
    provider::{CryptoFactory, CryptoType},
};

use super::{FeedProviderError, Price};

#[async_trait]
pub trait Provider {
    async fn get_spot_prices(&self, cosm_client: &Client) -> Result<Vec<Price>, FeedProviderError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Type {
    Crypto,
}

impl FromStr for Type {
    type Err = ();

    fn from_str(input: &str) -> Result<Type, Self::Err> {
        match input {
            "crypto" => Ok(Type::Crypto),
            _ => Err(()),
        }
    }
}

pub struct Factory;

impl Factory {
    pub fn new_provider(
        s: &Type,
        cfg: &configuration::Providers,
    ) -> Result<Box<dyn Provider>, FeedProviderError> {
        match s {
            Type::Crypto => {
                let provider_type = CryptoType::from_str(&cfg.name)
                    .map_err(|_| FeedProviderError::UnsupportedProviderType(cfg.name.clone()))?;

                CryptoFactory::new_provider(&provider_type, &cfg.base_address)
            }
        }
    }
}

pub async fn get_supported_denom_pairs(
    cosm_client: &Client,
) -> Result<SupportedDenomPairsResponse, FeedProviderError> {
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
        provider::{Factory, Type},
    };

    const TEST_OSMOSIS_URL: &str = "https://lcd-osmosis.keplr.app/osmosis/gamm/v1beta1/";

    #[test]
    fn get_provider() {
        let t = Type::from_str("crypto").unwrap();

        assert_eq!(t, Type::Crypto);

        Type::from_str("invalid").unwrap_err();

        Factory::new_provider(
            &Type::Crypto,
            &Providers {
                main_type: "crypto".to_string(),
                name: "osmosis".to_string(),
                base_address: TEST_OSMOSIS_URL.to_string(),
            },
        )
        .unwrap();
    }
}
