use std::{convert::identity, sync::Arc};

use async_trait::async_trait;
use reqwest::{
    Client as ReqwestClient, Error as ReqwestError, RequestBuilder, Response as ReqwestResponse,
    Url,
};
use serde::{Deserialize, Deserializer};
use thiserror::Error;
use tokio::task::JoinSet;
use toml::Value;

use chain_comms::{
    client::Client as NodeClient,
    interact::{error, query_wasm},
    reexport::tonic::transport::Channel as TonicChannel,
};

use crate::{
    config::{Currencies, EnvError, ProviderConfigExt, Symbol, Ticker},
    messages::{PoolId, QueryMsg, SupportedCurrencyPairsResponse, SwapLeg},
    provider::{ComparisonProvider, Price, Provider, ProviderError, ProviderSized},
};

pub(crate) struct Osmosis {
    http_client: ReqwestClient,
    prices_rpc_url: Url,
    nolus_node: Arc<NodeClient>,
    oracle_addr: Arc<str>,
    currencies: Currencies,
}

impl Osmosis {
    fn get_request_builder(&self, url_str: &str) -> Result<RequestBuilder, ProviderError> {
        self.prices_rpc_url
            .join(url_str)
            .map(|url: Url| self.http_client.get(url))
            .map_err(ProviderError::UrlOperationFailed)
    }

    async fn query_supported_currencies(
        &self,
        rpc: TonicChannel,
        oracle_addr: &str,
    ) -> Result<impl Iterator<Item = Route> + '_, error::WasmQuery> {
        query_wasm::<SupportedCurrencyPairsResponse>(
            rpc,
            oracle_addr,
            QueryMsg::SUPPORTED_CURRENCY_PAIRS,
        )
        .await
        .map(|swap_legs: Vec<SwapLeg>| {
            swap_legs
                .into_iter()
                .filter_map(|swap: SwapLeg| -> Option<Route> {
                    let from_symbol: String = self.currencies.0.get(&swap.from).cloned()?;
                    let to_symbol: String = self.currencies.0.get(&swap.to.target).cloned()?;

                    Some(Route {
                        pool_id: swap.to.pool_id,
                        from: TickerSymbol {
                            ticker: swap.from,
                            symbol: from_symbol,
                        },
                        to: TickerSymbol {
                            ticker: swap.to.target,
                            symbol: to_symbol,
                        },
                    })
                })
        })
    }

    async fn query_price(
        request_builder: RequestBuilder,
        from_symbol: Symbol,
        to_symbol: Symbol,
    ) -> Result<ReqwestResponse, ProviderError> {
        request_builder
            .query(&[
                ("base_asset_denom", from_symbol),
                ("quote_asset_denom", to_symbol),
            ])
            .send()
            .await
            .map_err(ProviderError::FetchPoolPrice)
    }

    async fn unwrap_response(
        response: ReqwestResponse,
        from_ticker: Ticker,
        to_ticker: Ticker,
    ) -> Result<Price, ReqwestError> {
        response.json().await.map(
            |AssetPrice {
                 spot_price:
                     Ratio {
                         numerator: base,
                         denominator: quote,
                     },
             }| Price::new(from_ticker, base, to_ticker, quote),
        )
    }
}

#[async_trait]
impl Provider for Osmosis {
    async fn get_prices(&self, fault_tolerant: bool) -> Result<Box<[Price]>, ProviderError> {
        let mut prices: Vec<Price> = Vec::new();

        {
            let mut set: JoinSet<Result<Price, ProviderError>> = JoinSet::new();

            for Route {
                pool_id,
                from:
                    TickerSymbol {
                        ticker: from_ticker,
                        symbol: from_symbol,
                    },
                to:
                    TickerSymbol {
                        ticker: to_ticker,
                        symbol: to_symbol,
                    },
            } in self
                .nolus_node
                .with_grpc(|rpc: TonicChannel| {
                    self.query_supported_currencies(rpc, &self.oracle_addr)
                })
                .await?
            {
                let request_builder_result: Result<RequestBuilder, ProviderError> =
                    self.get_request_builder(format!("pools/{pool_id}/prices").as_str());

                set.spawn(async {
                    let response: ReqwestResponse =
                        Self::query_price(request_builder_result?, from_symbol, to_symbol).await?;

                    if response.status().is_success() {
                        Self::unwrap_response(response, from_ticker, to_ticker)
                            .await
                            .map_err(ProviderError::DeserializePoolPrice)
                    } else {
                        Err(ProviderError::ServerResponse(
                            from_ticker,
                            to_ticker,
                            response.status().as_u16(),
                        ))
                    }
                });
            }

            while let Some(result) = set.join_next().await {
                match result.map_err(From::from).and_then(identity) {
                    Ok(price) => prices.push(price),
                    Err(error) if fault_tolerant => {
                        tracing::error!(error = %error, "Couldn't resolve price!")
                    }
                    Err(error) => return Err(error),
                }
            }
        }

        Ok(prices.into_boxed_slice())
    }
}

#[async_trait]
impl<Config> ProviderSized<Config> for Osmosis
where
    Config: ProviderConfigExt,
{
    const ID: &'static str = "osmosis";

    type ConstructError = ConstructError;

    async fn from_config(
        id: &str,
        config: &Config,
        oracle_addr: &Arc<str>,
        nolus_node: &Arc<NodeClient>,
    ) -> Result<Self, Self::ConstructError>
    where
        Self: Sized,
    {
        config
            .misc()
            .get("currencies")
            .ok_or(ConstructError::MissingField("currencies"))
            .cloned()
            .and_then(|value: Value| {
                value.try_into().map_err(|error: toml::de::Error| {
                    ConstructError::DeserializeField("currencies", error)
                })
            })
            .and_then(|currencies: Currencies| {
                Config::fetch_from_env(id, "RPC_URL")
                    .map_err(ConstructError::FetchPricesRpcUrl)
                    .and_then(|prices_rpc_url: String| {
                        Url::parse(&prices_rpc_url).map_err(ConstructError::InvalidPricesRpcUrl)
                    })
                    .map(|prices_rpc_url: Url| Self {
                        http_client: ReqwestClient::new(),
                        prices_rpc_url,
                        nolus_node: nolus_node.clone(),
                        oracle_addr: oracle_addr.clone(),
                        currencies,
                    })
            })
    }
}

#[async_trait]
impl ComparisonProvider for Osmosis {}

#[derive(Debug, Error)]
pub(crate) enum ConstructError {
    #[error("Missing \"{0}\" field in configuration file!")]
    MissingField(&'static str),
    #[error("Failed to deserialize field \"{0}\"! Cause: {1}")]
    DeserializeField(&'static str, toml::de::Error),
    #[error("Failed to fetch prices RPC's URL from environment variables! Cause: {0}")]
    FetchPricesRpcUrl(#[from] EnvError),
    #[error("Failed to parse prices RPC's URL! Cause: {0}")]
    InvalidPricesRpcUrl(#[from] url::ParseError),
}

struct Route {
    pool_id: PoolId,
    from: TickerSymbol,
    to: TickerSymbol,
}

struct TickerSymbol {
    ticker: Ticker,
    symbol: Symbol,
}

#[derive(Debug, Deserialize)]
pub struct AssetPrice {
    spot_price: Ratio,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Ratio {
    numerator: u128,
    denominator: u128,
}

impl<'de> Deserialize<'de> for Ratio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let point: usize;

        let spot_price: String = {
            let mut spot_price: String = String::deserialize(deserializer)?;

            point = spot_price
                .find('.')
                .map_or(spot_price.len(), |point: usize| -> usize {
                    spot_price = spot_price.trim_end_matches('0').into();

                    spot_price.remove(point);

                    point
                });

            spot_price
        };

        Ok(Ratio {
            numerator: 10_u128
                .checked_pow(
                    (spot_price.len() - point)
                        .try_into()
                        .map_err(serde::de::Error::custom)?,
                )
                .ok_or_else(|| {
                    serde::de::Error::custom("Couldn't calculate ratio! Exponent too big!")
                })?,
            denominator: spot_price
                .trim_start_matches('0')
                .parse()
                .map_err(serde::de::Error::custom)?,
        })
    }
}
