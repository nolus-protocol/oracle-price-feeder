use std::{
    collections::{BTreeMap, BTreeSet},
    convert::identity,
    string::FromUtf8Error,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{FutureExt, TryFutureExt};
use http::{HeaderMap, HeaderValue};
use regex::{Captures, Regex, RegexBuilder};
use reqwest::{Client as ReqwestClient, Error as ReqwestError, Response as ReqwestResponse};
use thiserror::Error;
use tokio::task::{block_in_place, JoinSet};
use toml::Value;

use chain_comms::client::Client as NodeClient;

use crate::{
    config::{self, ProviderConfigExt, Ticker, TickerUnsized},
    deviation,
    price::{self, Coin, CoinWithDecimalPlaces, CoinWithoutDecimalPlaces, Price, Ratio},
    provider::{ComparisonProvider, FromConfig, PriceComparisonGuardError},
};

pub(crate) struct SanityCheck {
    mandatory: bool,
    http_client: Arc<ReqwestClient>,
    ticker_mapping: BTreeMap<Arc<TickerUnsized>, Arc<str>>,
    supported_vs_currencies: BTreeSet<Arc<str>>,
}

impl SanityCheck {
    fn extract_mandatory_check_flag<Config>(config: &mut Config) -> Result<bool, ConstructError>
    where
        Config: ProviderConfigExt<true>,
    {
        const MANDATORY_CHECK_FIELD: &str = "mandatory";

        config
            .misc_mut()
            .remove(MANDATORY_CHECK_FIELD)
            .ok_or(ConstructError::MissingField(MANDATORY_CHECK_FIELD))
            .and_then(|value: Value| {
                value.try_into().map_err(|error: toml::de::Error| {
                    ConstructError::DeserializeField(MANDATORY_CHECK_FIELD, error)
                })
            })
    }

    fn construct_http_client<Config>(id: &str) -> Result<Arc<ReqwestClient>, ConstructError>
    where
        Config: ProviderConfigExt<true>,
    {
        const API_KEY_FIELD: &str = "api_key";
        const API_KEY_HEADER: &str = "x-cg-pro-api-key";

        ReqwestClient::builder()
            .default_headers({
                let mut headers: HeaderMap = HeaderMap::new();

                headers.insert(
                    API_KEY_HEADER,
                    Config::fetch_from_env(id, API_KEY_FIELD)
                        .map_err(ConstructError::EnvVariable)
                        .and_then(|api_key: String| {
                            HeaderValue::from_str(&api_key)
                                .map_err(ConstructError::ConstructApiKeyHeaderValue)
                        })?,
                );

                headers
            })
            .build()
            .map(Arc::new)
            .map_err(ConstructError::ConstructHttpClient)
    }

    fn extract_ticker_mapping<Config>(
        config: &mut Config,
    ) -> Result<BTreeMap<Arc<TickerUnsized>, Arc<str>>, ConstructError>
    where
        Config: ProviderConfigExt<true>,
    {
        const TICKER_MAPPING_FIELD: &str = "ticker_mapping";

        config
            .misc_mut()
            .remove(TICKER_MAPPING_FIELD)
            .ok_or(ConstructError::MissingField(TICKER_MAPPING_FIELD))
            .and_then(|value: Value| {
                value
                    .try_into()
                    .map(|mappings: BTreeMap<Ticker, String>| {
                        mappings
                            .into_iter()
                            .map(|(ticker, mapping): (Ticker, String)| {
                                (ticker.into(), mapping.into())
                            })
                            .collect()
                    })
                    .map_err(|error: toml::de::Error| {
                        ConstructError::DeserializeField(TICKER_MAPPING_FIELD, error)
                    })
            })
    }

    async fn fetch_supported_vs_currencies(
        http_client: &ReqwestClient,
        ticker_mappings: &BTreeMap<Arc<TickerUnsized>, Arc<str>>,
    ) -> Result<BTreeSet<Arc<str>>, ConstructError> {
        const SUPPORTED_VS_CURRENCIES_URL: &str =
            "https://pro-api.coingecko.com/api/v3/simple/supported_vs_currencies";

        http_client
            .get(SUPPORTED_VS_CURRENCIES_URL)
            .send()
            .map_err(ConstructError::SendSupportedVsCurrencies)
            .and_then(|response: ReqwestResponse| {
                response
                    .bytes()
                    .map_err(ConstructError::FetchSupportedVsCurrencies)
            })
            .map(|result: Result<Bytes, ConstructError>| {
                result.and_then(|body: Bytes| {
                    serde_json_wasm::from_slice(&body)
                        .map_err(ConstructError::DeserializeSupportedVsCurrencies)
                })
            })
            .map_ok(|currencies: BTreeSet<String>| {
                currencies
                    .into_iter()
                    .filter_map(|currency: String| {
                        ticker_mappings
                            .values()
                            .find(|mapping: &&Arc<str>| currency == mapping.as_ref())
                            .cloned()
                    })
                    .collect()
            })
            .await
    }

    fn get_mappings(&self, price: &Price<CoinWithDecimalPlaces>) -> Option<Mappings> {
        self.ticker_mapping
            .get_key_value(price.amount().ticker())
            .and_then(
                |(base_ticker, base_mapping): (&Arc<TickerUnsized>, &Arc<str>)| {
                    self.ticker_mapping
                        .get_key_value(price.amount_quote().ticker())
                        .and_then(
                            |(quote_ticker, quote_mapping): (&Arc<TickerUnsized>, &Arc<str>)| {
                                (self.supported_vs_currencies.contains(base_mapping.as_ref())
                                    || self
                                        .supported_vs_currencies
                                        .contains(quote_mapping.as_ref()))
                                .then_some(false)
                                .or_else(|| {
                                    self.supported_vs_currencies
                                        .contains(base_mapping.as_ref())
                                        .then_some(true)
                                })
                                .map(|inverted: bool| {
                                    let base: Mapping = Mapping {
                                        ticker: base_ticker.clone(),
                                        mapping: base_mapping.clone(),
                                    };

                                    let quote: Mapping = Mapping {
                                        ticker: quote_ticker.clone(),
                                        mapping: quote_mapping.clone(),
                                    };

                                    if inverted {
                                        Mappings {
                                            base: quote,
                                            quote: base,
                                        }
                                    } else {
                                        Mappings { base, quote }
                                    }
                                })
                            },
                        )
                },
            )
    }

    async fn query(
        http_client: Arc<ReqwestClient>,
        mappings: Mappings,
        regex: &'static Regex,
    ) -> Result<Price<CoinWithoutDecimalPlaces>, BenchmarkError> {
        const PRICE_URL: &str = "https://pro-api.coingecko.com/api/v3/simple/price";

        http_client
            .get(PRICE_URL)
            .query(&[
                ("ids", mappings.base.mapping.as_ref()),
                ("vs_currencies", mappings.quote.mapping.as_ref()),
                ("precision", "full"),
            ])
            .send()
            .map_err(BenchmarkError::SendQuery)
            .and_then(|response: ReqwestResponse| {
                response
                    .bytes()
                    .map_err(BenchmarkError::ReceiveResponseBody)
            })
            .map(|result: Result<Bytes, BenchmarkError>| {
                result.and_then(|body: Bytes| {
                    String::from_utf8({ body }.to_vec())
                        .map_err(BenchmarkError::InvalidUtf8)
                        .and_then(move |body| Self::parse_price_with_regex(&mappings, regex, body))
                })
            })
            .await
    }

    fn regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();

        REGEX.get_or_init(|| {
            let Ok(regex): Result<Regex, regex::Error> = RegexBuilder::new(
                r#"^\s*\{\s*"[\w\-]+"\s*:\s*\{\s*"[\w\-]*"\s*:\s*(\d+(?:\.\d+)?)\s*\}\s*\}\s*$"#,
            )
            .case_insensitive(true)
            .ignore_whitespace(false)
            .multi_line(true)
            .build() else {
                unreachable!()
            };

            regex
        })
    }

    fn parse_price_with_regex(
        mappings: &Mappings,
        regex: &Regex,
        body: String,
    ) -> Result<Price<CoinWithoutDecimalPlaces>, BenchmarkError> {
        let maybe_price: Option<Result<Price<CoinWithoutDecimalPlaces>, BenchmarkError>> = regex
            .captures(&body)
            .and_then(|captures: Captures<'_>| captures.get(1))
            .map(|price_decimal| {
                price_decimal
                    .as_str()
                    .parse()
                    .map(|price_ratio: Ratio| {
                        price_ratio.to_price(
                            mappings.base.ticker.to_string(),
                            mappings.quote.ticker.to_string(),
                        )
                    })
                    .map_err(BenchmarkError::ParsePrice)
            });

        maybe_price.unwrap_or_else(|| Err(BenchmarkError::PriceNotFoundInResponse(body)))
    }
}

struct Mapping {
    ticker: Arc<TickerUnsized>,
    mapping: Arc<str>,
}

struct Mappings {
    base: Mapping,
    quote: Mapping,
}

#[async_trait]
impl ComparisonProvider for SanityCheck {
    async fn benchmark_prices(
        &self,
        benchmarked_provider_id: &str,
        prices: &[Price<CoinWithDecimalPlaces>],
        max_deviation_exclusive: u64,
    ) -> Result<(), PriceComparisonGuardError> {
        let mut prices: Vec<Price<CoinWithDecimalPlaces>> = prices.to_vec();

        let mut comparison_prices: Vec<Price<CoinWithoutDecimalPlaces>> = Vec::new();

        let regex: &'static Regex = Self::regex();

        let mut set: JoinSet<Result<Price<CoinWithoutDecimalPlaces>, BenchmarkError>> =
            JoinSet::new();

        for index in (0..prices.len()).rev() {
            let price: &Price<CoinWithDecimalPlaces> = &prices[index];

            let Some(mappings): Option<Mappings> = self.get_mappings(price) else {
                let _: Price<CoinWithDecimalPlaces> = prices.remove(index);

                continue;
            };

            set.spawn(Self::query(self.http_client.clone(), mappings, regex));
        }

        if prices.is_empty() {
            debug_assert!(set.is_empty());

            if self.mandatory {
                tracing::error!(
                    "Sanity check failed for provider with ID: {id}! No intersection of prices is empty!",
                    id = benchmarked_provider_id,
                );

                Err(PriceComparisonGuardError::ComparisonProviderSpecific(
                    Box::new(BenchmarkError::EmptyPricesIntersection),
                ))
            } else {
                tracing::warn!(
                    "Sanity check unavailable for provider with ID: {id}! No intersection of prices is empty!",
                    id = benchmarked_provider_id,
                );

                Ok(())
            }
        } else {
            while let Some(result) = set.join_next().await {
                result
                    .map_err(BenchmarkError::JoinQueryTask)
                    .and_then(identity)
                    .map(|price: Price<CoinWithoutDecimalPlaces>| comparison_prices.push(price))
                    .map_err(|error: BenchmarkError| {
                        PriceComparisonGuardError::ComparisonProviderSpecific(Box::new(error))
                    })?;
            }

            let result: Result<(), PriceComparisonGuardError> = block_in_place(|| {
                deviation::compare_prices(&prices, &comparison_prices, max_deviation_exclusive)
            });

            if result.is_ok() {
                tracing::info!(
                    "Sanity check passed for provider with ID: {id}.",
                    id = benchmarked_provider_id,
                );
            } else {
                tracing::error!(
                    "Sanity check failed for provider with ID: {id}!",
                    id = benchmarked_provider_id,
                );
            }

            result
        }
    }
}

#[derive(Debug, Error)]
enum BenchmarkError {
    #[error("Failed sending price query! Cause: {0}")]
    SendQuery(ReqwestError),
    #[error("Failed to receive price query response body! Cause: {0}")]
    ReceiveResponseBody(ReqwestError),
    #[error("Failed to retrieve price from response! No price found! Raw response: {0}")]
    PriceNotFoundInResponse(String),
    #[error("Failed to parse response body as string! Cause: {0}")]
    InvalidUtf8(FromUtf8Error),
    #[error("Failed to parse price! Cause: {0}")]
    ParsePrice(price::Error),
    #[error("Failed to benchmark prices because intersection is empty!")]
    EmptyPricesIntersection,
    #[error("Failed to join price query task into main one! Cause: {0}")]
    JoinQueryTask(tokio::task::JoinError),
}

#[async_trait]
impl FromConfig<true> for SanityCheck {
    const ID: &'static str = "coin_gecko_sanity_check";

    type ConstructError = ConstructError;

    async fn from_config<Config>(
        id: &str,
        mut config: Config,
        _: &NodeClient,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<true>,
    {
        let mandatory: bool = Self::extract_mandatory_check_flag(&mut config)?;

        let http_client: Arc<ReqwestClient> = Self::construct_http_client::<Config>(id)?;

        let ticker_mapping: BTreeMap<Arc<TickerUnsized>, Arc<str>> =
            Self::extract_ticker_mapping(&mut config)?;

        if let Some(fields) = super::left_over_fields(config.into_misc()) {
            Err(ConstructError::UnknownFields(fields))
        } else {
            Self::fetch_supported_vs_currencies(&http_client, &ticker_mapping)
                .await
                .map(|supported_vs_currencies: BTreeSet<Arc<str>>| Self {
                    mandatory,
                    http_client,
                    ticker_mapping,
                    supported_vs_currencies,
                })
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum ConstructError {
    #[error("Failed to fetch value from environment! Cause: {0}")]
    EnvVariable(config::EnvError),
    #[error("Missing \"{0}\" field in configuration file!")]
    MissingField(&'static str),
    #[error("Failed to deserialize field \"{0}\"! Cause: {1}")]
    DeserializeField(&'static str, toml::de::Error),
    #[error("Failed to construct header value containing API key! Cause: {0}")]
    ConstructApiKeyHeaderValue(#[from] http::header::InvalidHeaderValue),
    #[error("Failed to construct HTTP client! Cause: {0}")]
    ConstructHttpClient(ReqwestError),
    #[error("Unknown fields found! Unknown fields: {0}")]
    UnknownFields(Box<str>),
    #[error("Failed to send \"supported versus currencies\"! Cause: {0}")]
    SendSupportedVsCurrencies(ReqwestError),
    #[error("Failed to fetch \"supported versus currencies\"! Cause: {0}")]
    FetchSupportedVsCurrencies(ReqwestError),
    #[error("Failed to deserialize \"supported versus currencies\"! Cause: {0}")]
    DeserializeSupportedVsCurrencies(serde_json_wasm::de::Error),
    #[error("Failed to parse prices RPC's URL! Cause: {0}")]
    InvalidPricesRpcUrl(#[from] url::ParseError),
}
