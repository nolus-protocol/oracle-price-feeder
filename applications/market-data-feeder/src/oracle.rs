use std::{future::Future, time::Duration};

use anyhow::{Context as _, Result};
use tokio::time::Instant;

use chain_ops::node::Reconnect;
use dex::{
    provider::{self},
    Currencies, CurrencyPairs,
};

pub struct Oracle<Dex>
where
    Dex: provider::Dex + ?Sized,
{
    oracle: oracle::Oracle<Dex>,
    last_update: Instant,
    update_interval: Duration,
    currencies: Currencies,
    currency_pairs: CurrencyPairs<Dex>,
}

impl<Dex> Oracle<Dex>
where
    Dex: provider::Dex + ?Sized,
{
    pub async fn new(
        mut oracle: oracle::Oracle<Dex>,
        update_interval: Duration,
    ) -> Result<Self> {
        let currencies = oracle
            .contract_mut()
            .query_currencies()
            .await
            .context("Failed to query currencies")?;

        let last_update = Instant::now();

        let currency_pairs = oracle
            .contract_mut()
            .query_currency_pairs()
            .await
            .context("Failed to query currency pairs!")?;

        Ok(Self {
            oracle,
            last_update,
            update_interval,
            currencies,
            currency_pairs,
        })
    }

    #[inline]
    #[must_use]
    pub const fn currencies(&self) -> &Currencies {
        &self.currencies
    }

    #[inline]
    #[must_use]
    pub const fn currency_pairs(&self) -> &CurrencyPairs<Dex> {
        &self.currency_pairs
    }

    pub async fn update_currencies_and_pairs(&mut self) -> Result<bool> {
        let update_interval_elapsed =
            self.last_update.elapsed() > self.update_interval;

        if update_interval_elapsed {
            let currencies = self
                .oracle
                .contract_mut()
                .query_currencies()
                .await
                .context("Failed to query currencies")?;

            let last_update = Instant::now();

            let currency_pairs = self
                .oracle
                .contract_mut()
                .query_currency_pairs()
                .await
                .context("Failed to query currency pairs")?;

            self.last_update = last_update;

            self.currencies = currencies;

            self.currency_pairs = currency_pairs;
        }

        Ok(update_interval_elapsed)
    }
}

impl<Dex> Reconnect for Oracle<Dex>
where
    Dex: provider::Dex,
{
    #[inline]
    fn reconnect(&self) -> impl Future<Output = Result<()>> + Send + '_ {
        self.oracle.reconnect()
    }
}

pub type PoolId = u64;
