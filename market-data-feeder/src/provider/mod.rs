use std::{error::Error as StdError, sync::Arc};

use async_trait::async_trait;
use futures::{FutureExt as _, TryFutureExt as _};
use serde::{Deserialize, Serialize};

use chain_comms::client::Client as NodeClient;

use crate::config::ProviderConfigExt;

pub(crate) use self::error::{
    PriceComparisonGuard as PriceComparisonGuardError, Provider as ProviderError,
};

mod error;

#[async_trait]
pub(crate) trait Provider: Sync + Send + 'static {
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
                        prices,
                        comparison_prices,
                        max_deviation_exclusive,
                    )
                    .await
                },
            )
            .await
    }
}

#[async_trait]
pub(crate) trait FromConfig: Sync + Send + Sized + 'static {
    const ID: &'static str;

    type ConstructError: StdError + Send + 'static;

    async fn from_config<Config>(
        id: &str,
        config: &Config,
        oracle_addr: &Arc<str>,
        nolus_client: &Arc<NodeClient>,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt;
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[must_use]
pub(crate) struct Coin {
    amount: u128,
    ticker: String,
}

impl Coin {
    pub const fn new(amount: u128, ticker: String) -> Self {
        Self { amount, ticker }
    }

    pub const fn amount(&self) -> u128 {
        self.amount
    }

    pub fn ticker(&self) -> &str {
        self.ticker.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[must_use]
pub(crate) struct Price {
    amount: Coin,
    amount_quote: Coin,
}

impl Price {
    pub const fn new(
        base_ticker: String,
        base_amount: u128,
        quote_ticker: String,
        quote_amount: u128,
    ) -> Self {
        Self::new_from_coins(
            Coin::new(base_amount, base_ticker),
            Coin::new(quote_amount, quote_ticker),
        )
    }

    pub const fn new_from_coins(amount: Coin, amount_quote: Coin) -> Self {
        Price {
            amount,
            amount_quote,
        }
    }

    pub const fn amount(&self) -> &Coin {
        &self.amount
    }

    pub const fn amount_quote(&self) -> &Coin {
        &self.amount_quote
    }
}
