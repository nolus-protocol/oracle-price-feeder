use std::{borrow::Cow, str::FromStr};

use async_trait::async_trait;

use crate::{
    configuration,
    cosmos::{Client, QueryMsg, SupportedDenomPairsResponse},
    provider::{CryptoFactory, CryptoType},
};

use super::{FeedProviderError, Price};

#[async_trait]
pub trait Provider
where
    Self: Send + 'static,
{
    fn name(&self) -> Cow<'static, str>;

    async fn get_spot_prices(
        &self,
        cosm_client: &Client,
    ) -> Result<Box<[Price]>, FeedProviderError>;
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
    ) -> Result<Box<dyn Provider + Send + 'static>, FeedProviderError> {
        match s {
            Type::Crypto => {
                let provider_type = CryptoType::from_str(&cfg.name)
                    .map_err(|_| FeedProviderError::UnsupportedProviderType(cfg.name.clone()))?;

                CryptoFactory::new_provider(&provider_type, &cfg.base_address, &cfg.currencies)
            }
        }
    }
}

pub async fn get_supported_denom_pairs(
    cosm_client: &Client,
) -> Result<SupportedDenomPairsResponse, FeedProviderError> {
    cosm_client
        .cosmwasm_query(&QueryMsg::SupportedCurrencyPairs {})
        .await
        .map_err(Into::into)
        .and_then(|resp| serde_json::from_slice(&resp.data).map_err(Into::into))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use crate::{
        configuration::Providers,
        provider::{Factory, Type},
    };

    const TEST_OSMOSIS_URL: &str = "https://lcd.osmosis.zone/gamm/v1beta1/";

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
                currencies: BTreeMap::from([
                    ("OSMO".into(), "OSMO".into()),
                    (
                        "ATOM".into(),
                        "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
                            .into(),
                    ),
                ]),
            },
        )
        .unwrap();
    }
}
