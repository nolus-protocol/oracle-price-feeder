use std::{
    collections::BTreeMap,
    env::{self, var},
    fmt::{Formatter, Result as FmtResult},
    sync::Arc,
    time::Duration,
};

use serde::{
    de::{Deserializer, Error as DeserializeError, MapAccess, Visitor},
    Deserialize,
};
use thiserror::Error as ThisError;

use chain_comms::config::Node;

pub(crate) type TickerUnsized = str;
pub(crate) type Ticker = String;

pub(crate) type Symbol = String;

pub(crate) type Currencies = BTreeMap<Ticker, Symbol>;

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct Config {
    pub tick_time: u64,
    #[serde(deserialize_with = "deserialize_providers_map")]
    pub providers: BTreeMap<String, ProviderWithComparison>,
    pub comparison_providers: BTreeMap<String, ComparisonProvider>,
    #[serde(deserialize_with = "deserialize_arc_str")]
    pub oracle_addr: Arc<str>,
    pub gas_limit: u64,
    pub node: Node,
}

impl AsRef<Node> for Config {
    fn as_ref(&self) -> &Node {
        &self.node
    }
}

pub(crate) trait ProviderConfig: Sync + Send {
    fn name(&self) -> &Arc<str>;

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

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub(crate) struct Provider {
    #[serde(deserialize_with = "deserialize_arc_str")]
    name: Arc<str>,
    #[serde(flatten)]
    misc: BTreeMap<String, toml::Value>,
}

impl ProviderConfig for Provider {
    fn name(&self) -> &Arc<str> {
        &self.name
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
    pub provider_id: String,
    pub max_deviation_exclusive: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub(crate) struct ComparisonProvider {
    #[serde(flatten)]
    pub provider: Provider,
}

fn deserialize_providers_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, ProviderWithComparison>, D::Error>
where
    D: Deserializer<'de>,
{
    struct V;

    impl<'de> Visitor<'de> for V {
        type Value = BTreeMap<String, ProviderWithComparison>;

        fn expecting(&self, formatter: &mut Formatter) -> FmtResult {
            formatter.write_str("price feed provider with optional comparison provider")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            #[derive(Deserialize)]
            #[serde(rename_all = "snake_case", deny_unknown_fields)]
            pub(crate) struct RawComparisonProviderId {
                pub provider_id: String,
            }

            impl RawComparisonProviderId {
                fn read_from_env_and_convert<E>(
                    self,
                    id: &str,
                ) -> Result<ComparisonProviderIdAndMaxDeviation, E>
                where
                    E: DeserializeError,
                {
                    <Provider as ProviderConfigExt<false>>::fetch_from_env(id, "max_deviation")
                        .map_err(E::custom)
                        .and_then(|value: String| {
                            value
                                .parse()
                                .map_err(E::custom)
                                .map(|max_deviation_exclusive: u64| {
                                    ComparisonProviderIdAndMaxDeviation {
                                        provider_id: self.provider_id,
                                        max_deviation_exclusive,
                                    }
                                })
                        })
                }
            }

            #[derive(Deserialize)]
            #[serde(rename_all = "snake_case")]
            struct RawProviderWithComparison {
                #[serde(flatten)]
                provider: Provider,
                comparison: Option<RawComparisonProviderId>,
            }

            let mut providers: BTreeMap<String, ProviderWithComparison> = BTreeMap::new();

            while let Some((
                id,
                RawProviderWithComparison {
                    comparison,
                    provider,
                },
            )) = map.next_entry::<String, RawProviderWithComparison>()?
            {
                let seconds_before_feeding: u64 =
                    <Provider as ProviderConfigExt<false>>::fetch_from_env(
                        &id,
                        "seconds_before_feeding",
                    )
                    .map_err(A::Error::custom)
                    .and_then(|value: String| value.parse().map_err(A::Error::custom))?;

                let comparison: Option<ComparisonProviderIdAndMaxDeviation> = comparison
                    .map(|comparison: RawComparisonProviderId| {
                        comparison.read_from_env_and_convert::<A::Error>(&id)
                    })
                    .transpose()?;

                providers.insert(
                    id,
                    ProviderWithComparison {
                        provider,
                        time_before_feeding: Duration::from_secs(seconds_before_feeding),
                        comparison,
                    },
                );
            }

            Ok(providers)
        }
    }

    deserializer.deserialize_map(V)
}

fn deserialize_arc_str<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Into::into)
}
