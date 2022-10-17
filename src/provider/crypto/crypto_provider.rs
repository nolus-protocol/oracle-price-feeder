use std::str::FromStr;

use crate::provider::{FeedProviderError, Provider};

use super::OsmosisClient;

#[derive(Debug, PartialEq, Eq)]
pub enum CryptoProviderType {
    Osmosis,
}

impl FromStr for CryptoProviderType {
    type Err = ();

    fn from_str(input: &str) -> Result<CryptoProviderType, Self::Err> {
        match input {
            "osmosis" => Ok(CryptoProviderType::Osmosis),
            _ => Err(()),
        }
    }
}

pub struct CryptoProvidersFactory;

impl CryptoProvidersFactory {
    pub fn new_provider(
        s: &CryptoProviderType,
        base_url: &str,
    ) -> Result<Box<dyn Provider>, FeedProviderError> {
        match s {
            CryptoProviderType::Osmosis => {
                OsmosisClient::new(base_url).map(|client| Box::new(client) as Box<dyn Provider>)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{CryptoProviderType, CryptoProvidersFactory};

    const TEST_OSMOSIS_URL: &str = "https://lcd-osmosis.keplr.app/osmosis/gamm/v1beta1/";

    #[test]
    fn get_crypto_provider() {
        let t = CryptoProviderType::from_str("osmosis").unwrap();
        assert_eq!(CryptoProviderType::Osmosis, t);
        CryptoProviderType::from_str("invalid").unwrap_err();
        CryptoProvidersFactory::new_provider(&CryptoProviderType::Osmosis, TEST_OSMOSIS_URL)
            .unwrap();
    }
}
