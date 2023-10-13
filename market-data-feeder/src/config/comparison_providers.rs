use std::{
    collections::btree_map::{BTreeMap, Entry as BTreeMapEntry},
    sync::Arc,
};

use serde::de::{Deserializer, Error as DeserializeError};

use super::{get_oracle, raw, str_pool::StrPool, ComparisonProvider, Provider};

pub(super) fn reconstruct<'r, 'de, D>(
    raw_comparison_providers: BTreeMap<String, raw::ComparisonProvider>,
    str_pool: &'r mut StrPool,
    oracles: &'r BTreeMap<String, Arc<str>>,
) -> Result<BTreeMap<Arc<str>, ComparisonProvider>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut comparison_providers: BTreeMap<Arc<str>, ComparisonProvider> = BTreeMap::new();

    for (
        raw_id,
        raw::ComparisonProvider {
            provider:
                raw::Provider {
                    name,
                    oracle_id,
                    misc,
                },
        },
    ) in raw_comparison_providers
    {
        let id: Arc<str> = str_pool.get_or_insert(raw_id);

        let comparison_provider: ComparisonProvider = ComparisonProvider {
            provider: Provider {
                name: str_pool.get_or_insert(name),
                oracle_addr: get_oracle::<D>(oracles, &oracle_id)?,
                misc,
            },
        };

        match comparison_providers.entry(id) {
            BTreeMapEntry::Vacant(entry) => entry.insert(comparison_provider),
            BTreeMapEntry::Occupied(entry) => {
                return Err(DeserializeError::custom(format_args!(
                    "Comparison provider with ID \"{id}\" already exists!",
                    id = entry.key()
                )))
            }
        };
    }

    Ok(comparison_providers)
}
