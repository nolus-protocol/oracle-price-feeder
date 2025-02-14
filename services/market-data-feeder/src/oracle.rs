use std::{collections::BTreeMap, future::Future, time::Duration};

use anyhow::{anyhow, Context as _, Result};
use serde::Deserialize;
use tokio::time::Instant;

use chain_ops::{
    contract::{Compatibility, SemVer},
    node::{QueryWasm, Reconnect},
};

pub struct Oracle {
    query_wasm: QueryWasm,
    address: String,
    last_update: Instant,
    update_interval: Duration,
    currencies: Currencies,
    currency_pairs: CurrencyPairs,
}

impl Oracle {
    pub async fn new(
        mut query_wasm: QueryWasm,
        address: String,
        update_interval: Duration,
    ) -> Result<Self> {
        const QUERY_MSG: &[u8; 23] = br#"{"contract_version":{}}"#;

        const CONTRACT_VERSION: SemVer = SemVer::new(0, 6, 0);

        query_wasm
            .smart(address.clone(), QUERY_MSG.to_vec())
            .await
            .and_then(|contract_version: String| contract_version.parse())
            .and_then(|version: SemVer| {
                match version.check_compatibility(CONTRACT_VERSION) {
                    Compatibility::Compatible => Ok(()),
                    Compatibility::Incompatible => Err(anyhow!(
                        "Oracle contract has an incompatible version!",
                    )),
                }
            })?;

        let currencies =
            Self::query_currencies(&mut query_wasm, address.clone())
                .await
                .context("Failed to query currencies")?;

        let last_update = Instant::now();

        let currency_pairs =
            Self::query_currency_pairs(&mut query_wasm, address.clone())
                .await
                .context("Failed to query currency pairs!")?;

        Ok(Self {
            query_wasm,
            address,
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
    pub const fn currency_pairs(&self) -> &CurrencyPairs {
        &self.currency_pairs
    }

    pub async fn update_currencies_and_pairs(&mut self) -> Result<bool> {
        let update_interval_elapsed =
            self.last_update.elapsed() > self.update_interval;

        if update_interval_elapsed {
            let currencies = Self::query_currencies(
                &mut self.query_wasm,
                self.address.clone(),
            )
            .await?;

            let last_update = Instant::now();

            let currency_pairs = Self::query_currency_pairs(
                &mut self.query_wasm,
                self.address.clone(),
            )
            .await?;

            self.last_update = last_update;

            self.currencies = currencies;

            self.currency_pairs = currency_pairs;
        }

        Ok(update_interval_elapsed)
    }

    async fn query_currencies(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> Result<Currencies> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct Currency {
            ticker: String,
            dex_symbol: String,
            decimal_digits: u8,
        }

        type Currencies = Vec<Currency>;

        const QUERY_MESSAGE: &[u8; 17] = br#"{"currencies":{}}"#;

        query_wasm
            .smart::<Currencies>(address, QUERY_MESSAGE.to_vec())
            .await
            .map(|currencies| {
                currencies
                    .into_iter()
                    .map(
                        |Currency {
                             ticker,
                             dex_symbol,
                             decimal_digits,
                         }| {
                            (
                                ticker,
                                self::Currency {
                                    dex_symbol,
                                    decimal_digits,
                                },
                            )
                        },
                    )
                    .collect()
            })
            .map(self::Currencies)
            .context("Failed to query for oracle contract currencies!")
    }

    async fn query_currency_pairs(
        query_wasm: &mut QueryWasm,
        address: String,
    ) -> Result<CurrencyPairs> {
        type FromTicker = String;

        type ToTicker = String;

        type PoolId = u64;

        type CurrencyPairs = Vec<(FromTicker, (PoolId, ToTicker))>;

        const QUERY_MESSAGE: &[u8; 31] = br#"{"supported_currency_pairs":{}}"#;

        query_wasm
            .smart::<CurrencyPairs>(address, QUERY_MESSAGE.to_vec())
            .await
            .map(|currency_pairs| {
                currency_pairs
                    .into_iter()
                    .map(|(from, (pool_id, to))| ((from, to), pool_id))
                    .collect()
            })
            .map(self::CurrencyPairs)
            .context(
                "Failed to query for oracle contract's configured currency \
                pairs!",
            )
    }
}

impl Reconnect for Oracle {
    #[inline]
    fn reconnect(&self) -> impl Future<Output = Result<()>> + Send + '_ {
        self.query_wasm.reconnect()
    }
}

#[repr(transparent)]
pub struct Currencies(BTreeMap<String, Currency>);

impl Currencies {
    pub fn get(&self, currency: &str) -> Result<&Currency> {
        self.0.get(currency).context(format!(
            r#"Currency "{currency}" which appears in the swap pairs was not
            reported when currencies were queried!"#
        ))
    }
}

pub struct Currency {
    pub dex_symbol: String,
    pub decimal_digits: u8,
}

#[repr(transparent)]
pub struct CurrencyPairs(BTreeMap<(String, String), PoolId>);

impl CurrencyPairs {
    #[inline]
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (&(String, String), &PoolId)> + Send + '_ {
        self.0.iter()
    }

    #[inline]
    pub fn keys(&self) -> impl Iterator<Item = &(String, String)> + Send + '_ {
        self.0.keys()
    }
}

pub type PoolId = u64;
