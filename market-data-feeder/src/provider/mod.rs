use std::{error::Error as StdError, sync::Arc};

use async_trait::async_trait;
use futures::{FutureExt as _, TryFutureExt as _};

use chain_comms::client::Client as NodeClient;

use crate::{
    config::{ProviderConfig, ProviderConfigExt},
    price::Price,
};

pub(crate) use self::error::{
    PriceComparisonGuard as PriceComparisonGuardError, Provider as ProviderError,
};

mod error;

#[async_trait]
pub(crate) trait Provider: Sync + Send + 'static {
    fn instance_id(&self) -> &str;

    async fn get_prices(&self, fault_tolerant: bool) -> Result<Box<[Price]>, ProviderError>;
}

#[async_trait]
pub(crate) trait ComparisonProvider: Sync + Send + 'static {
    async fn benchmark_prices(
        &self,
        benchmarked_provider: &dyn Provider,
        max_deviation_exclusive: u64,
    ) -> Result<(), PriceComparisonGuardError>;
}

#[async_trait]
impl<T> ComparisonProvider for T
where
    T: Provider + ?Sized,
{
    async fn benchmark_prices(
        &self,
        benchmarked_provider: &dyn Provider,
        max_deviation_exclusive: u64,
    ) -> Result<(), PriceComparisonGuardError> {
        self.get_prices(false)
            .map(|result: Result<Box<[Price]>, ProviderError>| {
                result.map_err(PriceComparisonGuardError::FetchPrices)
            })
            .and_then(|prices: Box<[Price]>| {
                benchmarked_provider.get_prices(false).map(
                    |result: Result<Box<[Price]>, ProviderError>| {
                        result
                            .map(|comparison_prices: Box<[Price]>| (prices, comparison_prices))
                            .map_err(PriceComparisonGuardError::FetchComparisonPrices)
                    },
                )
            })
            .and_then(
                |(prices, comparison_prices): (Box<[Price]>, Box<[Price]>)| async move {
                    crate::deviation::compare_prices(
                        &prices,
                        &comparison_prices,
                        max_deviation_exclusive,
                    )
                    .await
                },
            )
            .await
    }
}

#[async_trait]
pub(crate) trait FromConfig<const COMPARISON: bool>: Sync + Send + Sized + 'static {
    const ID: &'static str;

    type ConstructError: StdError + Send + 'static;

    async fn from_config<Config>(
        id: &str,
        config: &Config,
        oracle_addr: &Arc<str>,
        nolus_client: &Arc<NodeClient>,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<COMPARISON>;
}

#[async_trait]
impl<T: FromConfig<false>> FromConfig<true> for T {
    const ID: &'static str = T::ID;

    type ConstructError = T::ConstructError;

    async fn from_config<Config>(
        id: &str,
        config: &Config,
        oracle_addr: &Arc<str>,
        nolus_client: &Arc<NodeClient>,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<true>,
    {
        <T as FromConfig<false>>::from_config(id, &F(config), oracle_addr, nolus_client).await
    }
}

struct F<'r, Config: ProviderConfigExt<true>>(&'r Config);

impl<'r, Config: ProviderConfigExt<true>> ProviderConfig for F<'r, Config> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn misc(&self) -> &std::collections::BTreeMap<String, toml::Value> {
        self.0.misc()
    }
}

impl<'r, Config: ProviderConfigExt<true>> ProviderConfigExt<false> for F<'r, Config> {
    fn fetch_from_env(id: &str, name: &str) -> Result<String, crate::config::EnvError> {
        Config::fetch_from_env(id, name)
    }
}
