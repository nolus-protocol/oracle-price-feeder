use std::{
    collections::BTreeMap,
    env::{self, var},
    sync::Arc,
    time::Duration,
};

use serde::{
    de::{Deserializer, Error as DeserializeError},
    Deserialize,
};
use thiserror::Error as ThisError;

use chain_comms::config::Node;

use self::str_pool::StrPool;

mod comparison_providers;
mod providers;
mod raw;
mod str_pool;

pub(crate) type TickerUnsized = str;
pub(crate) type Ticker = String;

pub(crate) type SymbolUnsized = str;

pub(crate) type Currencies = BTreeMap<Ticker, SymbolAndDecimalPlaces>;

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct SymbolAndDecimalPlaces {
    #[serde(deserialize_with = "deserialize_arc_str")]
    denom: Arc<SymbolUnsized>,
    decimal_places: u8,
}

impl SymbolAndDecimalPlaces {
    pub const fn denom(&self) -> &Arc<SymbolUnsized> {
        &self.denom
    }

    pub const fn decimal_places(&self) -> u8 {
        self.decimal_places
    }
}

#[derive(Debug)]
#[must_use]
pub(crate) struct Config {
    pub tick_time: u64,
    pub oracles: Vec<Arc<str>>,
    pub providers: BTreeMap<Arc<str>, ProviderWithComparison>,
    pub comparison_providers: BTreeMap<Arc<str>, ComparisonProvider>,
    pub hard_gas_limit: u64,
    pub node: Node,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut str_pool: StrPool = StrPool::new();

        let raw::Config {
            tick_time,
            oracles: raw_oracles,
            providers: raw_providers,
            comparison_providers: raw_comparison_providers,
            hard_gas_limit,
            node,
        }: raw::Config = raw::Config::deserialize(deserializer)?;

        let mut oracles: BTreeMap<String, Arc<str>> = BTreeMap::new();

        for (raw_oracle_id, raw_oracle_addr) in raw_oracles {
            #[cfg(debug_assertions)]
            let None: Option<Arc<str>> =
                oracles.insert(raw_oracle_id, str_pool.get_or_insert(raw_oracle_addr))
            else {
                unreachable!()
            };
            #[cfg(not(debug_assertions))]
            oracles.insert(raw_oracle_id, str_pool.get_or_insert(raw_oracle_addr));
        }

        let comparison_providers: BTreeMap<Arc<str>, ComparisonProvider> =
            comparison_providers::reconstruct::<D>(
                raw_comparison_providers,
                &mut str_pool,
                &oracles,
            )?;

        let providers = providers::reconstruct::<D>(raw_providers, str_pool, &oracles)?;

        Ok(Config {
            tick_time,
            oracles: oracles.into_values().collect(),
            providers,
            comparison_providers,
            hard_gas_limit,
            node,
        })
    }
}

fn get_oracle<'r, 'de, D>(
    oracles: &'r BTreeMap<String, Arc<str>>,
    oracle_id: &str,
) -> Result<Arc<str>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(oracles
        .get(oracle_id)
        .ok_or_else(|| {
            DeserializeError::custom(format_args!("Unknown oracle ID: \"{oracle_id}\"!"))
        })?
        .clone())
}

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}

pub(crate) trait ProviderConfig: Sync + Send {
    fn name(&self) -> &Arc<str>;

    fn oracle_addr(&self) -> &Arc<str>;

    fn misc(&self) -> &BTreeMap<String, toml::Value>;

    fn misc_mut(&mut self) -> &mut BTreeMap<String, toml::Value>;

    fn into_misc(self) -> BTreeMap<String, toml::Value>;
}

pub(crate) trait ProviderConfigExt<const COMPARISON: bool>: ProviderConfig {
    fn fetch_from_env(id: &str, name: &str) -> Result<String, EnvError>;
}

impl<T> ProviderConfigExt<true> for T
where
    T: ProviderConfig + ?Sized,
{
    fn fetch_from_env(id: &str, name: &str) -> Result<String, EnvError> {
        let name: String = format!(
            "COMPARISON_PROVIDER_{id}_{field}",
            id = id.to_ascii_uppercase(),
            field = name.to_ascii_uppercase()
        );

        var(&name).map_err(|error: env::VarError| EnvError(name, error))
    }
}

#[derive(Debug, ThisError)]
#[error("Variable name: \"{0}\". Cause: {1}")]
pub(crate) struct EnvError(String, env::VarError);

#[derive(Debug, Clone)]
#[must_use]
pub(crate) struct Provider {
    name: Arc<str>,
    oracle_addr: Arc<str>,
    misc: BTreeMap<String, toml::Value>,
}

impl ProviderConfig for Provider {
    fn name(&self) -> &Arc<str> {
        &self.name
    }

    fn oracle_addr(&self) -> &Arc<str> {
        &self.oracle_addr
    }

    fn misc(&self) -> &BTreeMap<String, toml::Value> {
        &self.misc
    }

    fn misc_mut(&mut self) -> &mut BTreeMap<String, toml::Value> {
        &mut self.misc
    }

    fn into_misc(self) -> BTreeMap<String, toml::Value> {
        self.misc
    }
}

impl ProviderConfigExt<false> for Provider {
    fn fetch_from_env(id: &str, name: &str) -> Result<String, EnvError> {
        let name: String = format!(
            "PROVIDER_{id}_{field}",
            id = id.to_ascii_uppercase(),
            field = name.to_ascii_uppercase()
        );

        var(&name).map_err(|error: env::VarError| EnvError(name, error))
    }
}

#[derive(Debug, Clone)]
#[must_use]
pub(crate) struct ProviderWithComparison {
    pub provider: Provider,
    pub time_before_feeding: Duration,
    pub comparison: Option<ComparisonProviderIdAndMaxDeviation>,
}

#[derive(Debug, Clone)]
#[must_use]
pub(crate) struct ComparisonProviderIdAndMaxDeviation {
    pub provider_id: Arc<str>,
    pub max_deviation_exclusive: u64,
}

#[derive(Debug, Clone)]
#[repr(transparent)]
#[must_use]
pub(crate) struct ComparisonProvider {
    pub provider: Provider,
}

fn deserialize_arc_str<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Into::into)
}
