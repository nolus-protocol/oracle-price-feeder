use std::{collections::BTreeMap, str::FromStr};

use crate::{
    config::{Symbol, Ticker},
    provider::{FeedProviderError, Provider},
};

use super::Client;

#[derive(Debug, PartialEq, Eq)]
pub enum Type {
    Osmosis,
}

impl FromStr for Type {
    type Err = ();

    fn from_str(input: &str) -> Result<Type, Self::Err> {
        match input {
            "osmosis" => Ok(Type::Osmosis),
            _ => Err(()),
        }
    }
}

pub struct Factory;

impl Factory {
    pub fn new_provider(
        s: &Type,
        base_url: &str,
        currencies: &BTreeMap<Ticker, Symbol>,
    ) -> Result<Box<dyn Provider + Send + 'static>, FeedProviderError> {
        match s {
            Type::Osmosis => Client::new(base_url, currencies)
                .map(|client| Box::new(client) as Box<dyn Provider + Send + 'static>),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use super::{Factory, Type};

    const TEST_OSMOSIS_URL: &str = "https://lcd-osmosis.keplr.app/osmosis/gamm/v1beta1/";

    #[test]
    fn get_crypto_provider() {
        let t = Type::from_str("osmosis").unwrap();
        assert_eq!(t, Type::Osmosis);
        Type::from_str("invalid").unwrap_err();
        Factory::new_provider(
            &Type::Osmosis,
            TEST_OSMOSIS_URL,
            &BTreeMap::from([
                ("OSMO".into(), "OSMO".into()),
                (
                    "ATOM".into(),
                    "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2".into(),
                ),
            ]),
        )
        .unwrap();
    }
}
