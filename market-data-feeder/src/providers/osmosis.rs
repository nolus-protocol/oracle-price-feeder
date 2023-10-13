use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{
    Client as ReqwestClient, Error as ReqwestError, RequestBuilder, Response as ReqwestResponse,
    Url,
};
use serde::Deserialize;
use thiserror::Error;
use tokio::task::JoinSet;
use toml::Value;

use chain_comms::{
    client::Client as NodeClient,
    interact::{error, query_wasm},
    reexport::tonic::transport::Channel as TonicChannel,
};

use crate::{
    config::{
        Currencies, EnvError, ProviderConfigExt, SymbolAndDecimalPlaces, SymbolUnsized, Ticker,
    },
    messages::{PoolId, QueryMsg, SupportedCurrencyPairsResponse, SwapLeg},
    price::{CoinWithDecimalPlaces, Price, Ratio},
    provider::{FromConfig, Provider, ProviderError},
};

pub(crate) struct Osmosis {
    instance_id: String,
    http_client: ReqwestClient,
    prices_rpc_url: Url,
    nolus_node: NodeClient,
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
    ) -> Result<impl Iterator<Item = Route> + '_, error::WasmQuery> {
        query_wasm::<SupportedCurrencyPairsResponse>(
            rpc,
            self.oracle_addr.to_string(),
            QueryMsg::SUPPORTED_CURRENCY_PAIRS,
        )
        .await
        .map(|swap_legs: Vec<SwapLeg>| {
            swap_legs
                .into_iter()
                .filter_map(|swap: SwapLeg| -> Option<Route> {
                    let (from_symbol, from_decimal_places): (Arc<SymbolUnsized>, u8) = self
                        .currencies
                        .get(&swap.from)
                        .map(|symbol_and_decimal_places: &SymbolAndDecimalPlaces| {
                            (
                                symbol_and_decimal_places.denom().clone(),
                                symbol_and_decimal_places.decimal_places(),
                            )
                        })?;

                    let (to_symbol, to_decimal_places): (Arc<SymbolUnsized>, u8) = self
                        .currencies
                        .get(&swap.to.target)
                        .map(|symbol_and_decimal_places: &SymbolAndDecimalPlaces| {
                            (
                                symbol_and_decimal_places.denom().clone(),
                                symbol_and_decimal_places.decimal_places(),
                            )
                        })?;

                    Some(Route {
                        pool_id: swap.to.pool_id,
                        from: TickerSymbolDecimalPlaces {
                            ticker: swap.from,
                            symbol: from_symbol,
                            decimal_places: from_decimal_places,
                        },
                        to: TickerSymbolDecimalPlaces {
                            ticker: swap.to.target,
                            symbol: to_symbol,
                            decimal_places: to_decimal_places,
                        },
                    })
                })
        })
    }

    async fn query_price(
        request_builder: RequestBuilder,
        from_symbol: &SymbolUnsized,
        to_symbol: &SymbolUnsized,
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
        from_decimal_places: u8,
        to_ticker: Ticker,
        to_decimal_places: u8,
    ) -> Result<Price<CoinWithDecimalPlaces>, ReqwestError> {
        response
            .json()
            .await
            .map(|AssetPrice { spot_price }: AssetPrice| {
                spot_price.to_price_with_decimal_places(
                    from_ticker,
                    from_decimal_places,
                    to_ticker,
                    to_decimal_places,
                )
            })
    }
}

#[async_trait]
impl Provider for Osmosis {
    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    async fn get_prices(
        &self,
        fault_tolerant: bool,
    ) -> Result<Box<[Price<CoinWithDecimalPlaces>]>, ProviderError> {
        let mut set: JoinSet<Result<Price<CoinWithDecimalPlaces>, ProviderError>> = JoinSet::new();

        for Route {
            pool_id,
            from:
                TickerSymbolDecimalPlaces {
                    ticker: from_ticker,
                    symbol: from_symbol,
                    decimal_places: from_decimal_places,
                },
            to:
                TickerSymbolDecimalPlaces {
                    ticker: to_ticker,
                    symbol: to_symbol,
                    decimal_places: to_decimal_places,
                },
        } in self
            .nolus_node
            .with_grpc(|rpc: TonicChannel| self.query_supported_currencies(rpc))
            .await?
        {
            let request_builder_result: Result<RequestBuilder, ProviderError> =
                self.get_request_builder(format!("pools/{pool_id}/prices").as_str());

            set.spawn(async move {
                let response: ReqwestResponse =
                    Self::query_price(request_builder_result?, &from_symbol, &to_symbol).await?;

                if response.status().is_success() {
                    Self::unwrap_response(
                        response,
                        from_ticker,
                        from_decimal_places,
                        to_ticker,
                        to_decimal_places,
                    )
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

        super::collect_prices_from_task_set(set, fault_tolerant).await
    }
}

#[async_trait]
impl FromConfig<false> for Osmosis {
    const ID: &'static str = "osmosis";

    type ConstructError = ConstructError;

    async fn from_config<Config>(
        id: &str,
        mut config: Config,
        nolus_node: &NodeClient,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<false>,
    {
        config
            .misc_mut()
            .remove("currencies")
            .ok_or(ConstructError::MissingField("currencies"))
            .and_then(|value: Value| {
                value.try_into().map_err(|error: toml::de::Error| {
                    ConstructError::DeserializeField("currencies", error)
                })
            })
            .and_then(|currencies: Currencies| {
                let oracle_addr: Arc<str> = config.oracle_addr().clone();

                if let Some(fields) =
                    config
                        .into_misc()
                        .into_keys()
                        .reduce(|mut accumulator: String, key: String| {
                            accumulator.reserve(key.len() + 2);

                            accumulator.push_str(", ");

                            accumulator.push_str(&key);

                            accumulator
                        })
                {
                    Err(ConstructError::UnknownFields(fields.into_boxed_str()))
                } else {
                    Config::fetch_from_env(id, "RPC_URL")
                        .map_err(ConstructError::FetchPricesRpcUrl)
                        .and_then(|prices_rpc_url: String| {
                            Url::parse(&prices_rpc_url).map_err(ConstructError::InvalidPricesRpcUrl)
                        })
                        .map(|prices_rpc_url: Url| Self {
                            instance_id: id.to_string(),
                            http_client: ReqwestClient::new(),
                            prices_rpc_url,
                            nolus_node: nolus_node.clone(),
                            oracle_addr,
                            currencies,
                        })
                }
            })
    }
}

#[derive(Debug, Error)]
pub(crate) enum ConstructError {
    #[error("Missing \"{0}\" field in configuration file!")]
    MissingField(&'static str),
    #[error("Failed to deserialize field \"{0}\"! Cause: {1}")]
    DeserializeField(&'static str, toml::de::Error),
    #[error("Unknown fields found! Unknown fields: {0}")]
    UnknownFields(Box<str>),
    #[error("Failed to fetch prices RPC's URL from environment variables! Cause: {0}")]
    FetchPricesRpcUrl(#[from] EnvError),
    #[error("Failed to parse prices RPC's URL! Cause: {0}")]
    InvalidPricesRpcUrl(#[from] url::ParseError),
}

struct Route {
    pool_id: PoolId,
    from: TickerSymbolDecimalPlaces,
    to: TickerSymbolDecimalPlaces,
}

struct TickerSymbolDecimalPlaces {
    ticker: Ticker,
    symbol: Arc<SymbolUnsized>,
    decimal_places: u8,
}

#[derive(Debug, Deserialize)]
pub struct AssetPrice {
    spot_price: Ratio,
}
