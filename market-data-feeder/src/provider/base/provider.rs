use std::{borrow::Cow, str::FromStr};

use async_trait::async_trait;

use chain_comms::client::Client as NodeClient;

use crate::{
    config,
    provider::{CryptoFactory, CryptoType},
};

use super::{FeedProviderError, InvalidProviderType, Price};

#[async_trait]
pub trait Provider
where
    Self: Send + 'static,
{
    fn name(&self) -> Cow<'static, str>;

    async fn get_spot_prices(
        &self,
        node_client: &NodeClient,
        oracle_addr: &str,
    ) -> Result<Box<[Price]>, FeedProviderError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Type {
    Crypto,
}

impl FromStr for Type {
    type Err = InvalidProviderType;

    fn from_str(input: &str) -> Result<Type, Self::Err> {
        match input {
            "crypto" => Ok(Type::Crypto),
            _ => Err(InvalidProviderType::new(input.into())),
        }
    }
}

pub struct Factory;

impl Factory {
    pub fn new_provider(
        s: &Type,
        cfg: &config::Provider,
    ) -> Result<Box<dyn Provider + Send + 'static>, FeedProviderError> {
        match s {
            Type::Crypto => {
                let provider_type = CryptoType::from_str(&cfg.api_info.name).map_err(|_| {
                    FeedProviderError::UnsupportedProviderType(cfg.api_info.name.clone())
                })?;

                CryptoFactory::new_provider(
                    &provider_type,
                    &cfg.api_info.base_address,
                    &cfg.currencies,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use crate::config::ApiInfo;
    use crate::{
        config::Provider,
        provider::{Factory, Type},
    };

    const TEST_OSMOSIS_URL: &str = "https://lcd.osmosis.zone/osmosis/gamm/v1beta1/";

    #[test]
    fn get_provider() {
        let t = Type::from_str("crypto").unwrap();

        assert_eq!(t, Type::Crypto);

        Type::from_str("invalid").unwrap_err();

        Factory::new_provider(
            &Type::Crypto,
            &Provider {
                main_type: "crypto".to_string(),
                api_info: ApiInfo {
                    name: "osmosis".to_string(),
                    base_address: TEST_OSMOSIS_URL.to_string(),
                },
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
