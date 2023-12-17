use std::{collections::BTreeMap, error::Error as StdError, sync::Arc};

use async_trait::async_trait;
use futures::{FutureExt as _, TryFutureExt as _};
use tracing::{error, info};

use chain_comms::client::Client as NodeClient;

use crate::{
    config::{ProviderConfig, ProviderConfigExt},
    price::{CoinWithDecimalPlaces, Price},
};

pub(crate) use self::error::{
    PriceComparisonGuard as PriceComparisonGuardError, Provider as ProviderError,
};

mod error;

#[async_trait]
pub(crate) trait Provider: Sync + Send + 'static {
    fn instance_id(&self) -> &str;

    async fn get_prices(
        &self,
        fault_tolerant: bool,
    ) -> Result<Box<[Price<CoinWithDecimalPlaces>]>, ProviderError>;
}

#[async_trait]
pub(crate) trait ComparisonProvider: Sync + Send + 'static {
    async fn benchmark_prices(
        &self,
        benchmarked_provider_id: &str,
        prices: &[Price<CoinWithDecimalPlaces>],
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
        benchmarked_provider_id: &str,
        prices: &[Price<CoinWithDecimalPlaces>],
        max_deviation_exclusive: u64,
    ) -> Result<(), PriceComparisonGuardError> {
        self.get_prices(false)
            .map(|result: Result<Box<[Price<CoinWithDecimalPlaces>]>, ProviderError>| {
                result.map_err(PriceComparisonGuardError::FetchPrices)
            })
            .and_then(|comparison_prices: Box<[Price<CoinWithDecimalPlaces>]>| async move {
                let result: Result<(), PriceComparisonGuardError> = crate::deviation::compare_prices(
                    prices,
                    &comparison_prices,
                    max_deviation_exclusive,
                )
                .await;

                match &result {
                    Ok(()) => info!("Price comparison guard check of \"{benchmarked_provider_id}\" passed against \"{self_id}\".", self_id = self.instance_id()),
                    Err(error) => error!(error = ?error, "Price comparison guard check of \"{benchmarked_provider_id}\" failed against \"{self_id}\"! Cause: {error}", self_id = self.instance_id()),
                }

                result
            })
            .await
    }
}

#[async_trait]
pub(crate) trait FromConfig<const COMPARISON: bool>: Sync + Send + Sized + 'static {
    const ID: &'static str;

    type ConstructError: StdError + Send + 'static;

    async fn from_config<Config>(
        id: &str,
        config: Config,
        nolus_client: &NodeClient,
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
        config: Config,
        nolus_client: &NodeClient,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<true>,
    {
        <T as FromConfig<false>>::from_config(id, ProviderConfigWrapper(config), nolus_client).await
    }
}

struct ProviderConfigWrapper<Config: ProviderConfigExt<true>>(Config);

impl<Config: ProviderConfigExt<true>> ProviderConfig for ProviderConfigWrapper<Config> {
    fn name(&self) -> &Arc<str> {
        self.0.name()
    }

    fn oracle_name(&self) -> &Arc<str> {
        self.0.oracle_name()
    }

    fn oracle_addr(&self) -> &Arc<str> {
        self.0.oracle_addr()
    }

    fn misc(&self) -> &BTreeMap<String, toml::Value> {
        self.0.misc()
    }

    fn misc_mut(&mut self) -> &mut BTreeMap<String, toml::Value> {
        self.0.misc_mut()
    }

    fn into_misc(self) -> BTreeMap<String, toml::Value> {
        self.0.into_misc()
    }
}

impl<Config: ProviderConfigExt<true>> ProviderConfigExt<false> for ProviderConfigWrapper<Config> {
    fn fetch_from_env(id: &str, name: &str) -> Result<String, crate::config::EnvError> {
        Config::fetch_from_env(id, name)
    }
}
