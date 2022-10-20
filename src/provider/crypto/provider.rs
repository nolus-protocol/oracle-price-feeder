use std::str::FromStr;

use crate::provider::{FeedProviderError, Provider};

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
    pub fn new_provider(s: &Type, base_url: &str) -> Result<Box<dyn Provider>, FeedProviderError> {
        match s {
            Type::Osmosis => {
                Client::new(base_url).map(|client| Box::new(client) as Box<dyn Provider>)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Factory, Type};

    const TEST_OSMOSIS_URL: &str = "https://lcd-osmosis.keplr.app/osmosis/gamm/v1beta1/";

    #[test]
    fn get_crypto_provider() {
        let t = Type::from_str("osmosis").unwrap();
        assert_eq!(t, Type::Osmosis);
        Type::from_str("invalid").unwrap_err();
        Factory::new_provider(&Type::Osmosis, TEST_OSMOSIS_URL).unwrap();
    }
}
