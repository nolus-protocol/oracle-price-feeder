use std::{
    collections::btree_map::{BTreeMap, Entry as BTreeMapEntry},
    sync::Arc,
};

use serde::de::{Deserializer, Error as DeserializeError};

use super::{
    get_oracle, raw, str_pool::StrPool, ComparisonProviderIdAndMaxDeviation,
    Provider, ProviderConfigExt, ProviderWithComparison,
};

pub(super) fn reconstruct<'r, 'de, D>(
    raw_providers: BTreeMap<String, raw::ProviderWithComparison>,
    mut str_pool: StrPool,
    oracles: &'r BTreeMap<Arc<str>, Arc<str>>,
) -> Result<BTreeMap<Box<str>, ProviderWithComparison>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut providers: BTreeMap<Box<str>, ProviderWithComparison> =
        BTreeMap::new();

    for (
        raw_id,
        raw::ProviderWithComparison {
            provider:
                raw::Provider {
                    name,
                    oracle_id,
                    misc,
                },
            comparison,
        },
    ) in raw_providers
    {
        let id: Box<str> = raw_id.into_boxed_str();

        let oracle_id: Arc<str> = str_pool.get_or_insert(oracle_id);
        let oracle_address: Arc<str> = get_oracle::<D>(oracles, &oracle_id)?;

        let provider: ProviderWithComparison = ProviderWithComparison {
            provider: Provider {
                name: str_pool.get_or_insert(name),
                oracle_id,
                oracle_address,
                misc,
            },
            comparison: map_comparison_provider_option::<D>(
                comparison,
                &id,
                &mut str_pool,
            )?,
        };

        match providers.entry(id) {
            BTreeMapEntry::Vacant(entry) => entry.insert(provider),
            BTreeMapEntry::Occupied(entry) => {
                return Err(DeserializeError::custom(format_args!(
                    "Provider with ID \"{id}\" already exists!",
                    id = entry.key()
                )));
            },
        };
    }

    Ok(providers)
}

fn map_comparison_provider_option<'de, D>(
    comparison: Option<raw::ComparisonProviderIdAndMaxDeviation>,
    id: &str,
    str_pool: &mut StrPool,
) -> Result<Option<ComparisonProviderIdAndMaxDeviation>, D::Error>
where
    D: Deserializer<'de>,
{
    comparison
        .map(|raw::ComparisonProviderIdAndMaxDeviation { provider_id }: raw::ComparisonProviderIdAndMaxDeviation| {
            <Provider as ProviderConfigExt<false>>::fetch_from_env(id, "max_deviation")
                .map_err(DeserializeError::custom)
                .and_then(|value: String| {
                    value.parse().map_err(DeserializeError::custom)
                })
                .map(|max_deviation_exclusive: u64| {
                    ComparisonProviderIdAndMaxDeviation {
                        provider_id: str_pool.get_or_insert(provider_id),
                        max_deviation_exclusive,
                    }
                })
        })
        .transpose()
}
