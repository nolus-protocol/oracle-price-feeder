use std::collections::BTreeMap;

use serde::Deserialize;

use chain_comms::config::Node as NodeConfig;

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub(super) struct Provider {
    pub name: String,
    pub oracle_id: String,
    #[serde(flatten)]
    pub misc: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case")]
pub(super) struct ProviderWithComparison {
    #[serde(flatten)]
    pub provider: Provider,
    pub comparison: Option<ComparisonProviderIdAndMaxDeviation>,
}

#[derive(Debug, Clone, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) struct ComparisonProviderIdAndMaxDeviation {
    pub provider_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[repr(transparent)]
#[must_use]
#[serde(transparent, rename_all = "snake_case", deny_unknown_fields)]
pub(super) struct ComparisonProvider {
    pub provider: Provider,
}

#[derive(Debug, Deserialize)]
#[must_use]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) struct Config {
    pub tick_seconds: u64,
    pub between_tx_margin_seconds: u64,
    pub query_delivered_tx_tick_seconds: u64,
    pub hard_gas_limit: u64,
    pub node: NodeConfig,
    pub oracles: BTreeMap<String, String>,
    pub providers: BTreeMap<String, ProviderWithComparison>,
    pub comparison_providers: BTreeMap<String, ComparisonProvider>,
}
