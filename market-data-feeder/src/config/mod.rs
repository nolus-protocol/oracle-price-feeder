use std::{
    collections::BTreeMap,
    env::{var, VarError},
};

use serde::{de::Error as _, Deserialize, Deserializer};

use chain_comms::config::Node;

mod currencies;

pub type Ticker = String;

pub type Symbol = String;

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Config {
    continuous: bool,
    tick_time: u64,
    providers: Vec<Provider>,
    oracle_addr: String,
    gas_limit: u64,
    node: Node,
}

impl Config {
    #[cfg(test)]
    pub fn new(
        continuous: bool,
        tick_time: u64,
        providers: Vec<Provider>,
        oracle_addr: String,
        gas_limit: u64,
        node: Node,
    ) -> Self {
        Self {
            continuous,
            tick_time,
            providers,
            oracle_addr,
            gas_limit,
            node,
        }
    }

    pub fn continuous(&self) -> bool {
        self.continuous
    }
    pub fn tick_time(&self) -> u64 {
        self.tick_time
    }
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }
    pub fn oracle_addr(&self) -> &str {
        &self.oracle_addr
    }
    pub fn gas_limit(&self) -> u64 {
        self.gas_limit
    }
}

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Provider {
    pub main_type: String,
    #[serde(flatten)]
    pub api_info: ApiInfo,
    #[serde(with = "currencies::serde")]
    pub currencies: BTreeMap<Ticker, Symbol>,
}

#[derive(Debug)]
pub struct ApiInfo {
    pub name: String,
    pub base_address: String,
}

impl<'de> Deserialize<'de> for ApiInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct ProviderName {
            pub name: String,
        }

        let name: String = ProviderName::deserialize(deserializer)?.name;

        env_var_string::<D>(&format!(
            "PROVIDER_{}_BASE_ADDRESS",
            name.to_ascii_uppercase()
        ))
        .map(|base_address: String| ApiInfo { name, base_address })
    }
}

fn env_var_string<'de, 'var, D>(var_name: &'var str) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match var(var_name) {
        Ok(value) => Ok(value),
        Err(VarError::NotPresent) => Err(D::Error::custom(format!(
            r#"Missing environment variable: "{var_name}"!"#
        ))),
        Err(VarError::NotUnicode(_)) => Err(D::Error::custom(format!(
            r#"Environment variable "{var_name}" contains invalid unicode data!"#
        ))),
    }
}
