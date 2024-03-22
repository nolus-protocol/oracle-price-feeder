use std::sync::Arc;

use async_trait::async_trait;
use osmosis_std::types::osmosis::poolmanager::v2::{
    SpotPriceRequest, SpotPriceResponse,
};
use thiserror::Error;
use tokio::task::JoinSet;
use toml::Value;
use tracing::debug;

use chain_comms::{
    client::{self, Client as NodeClient},
    interact::query,
    reexport::{
        cosmrs::proto::cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient,
        tonic::transport::Channel as TonicChannel,
    },
};

use crate::{
    config::{
        Currencies, EnvError, ProviderConfigExt, SymbolAndDecimalPlaces,
        SymbolUnsized, Ticker,
    },
    messages::{PoolId, QueryMsg, SupportedCurrencyPairsResponse, SwapLeg},
    price::{CoinWithDecimalPlaces, Price, Ratio},
    provider::{FromConfig, Provider, ProviderError},
};

pub(crate) struct Osmosis {
    instance_id: String,
    node_client: NodeClient,
    oracle_addr: Arc<str>,
    channel: TonicChannel,
    currencies: Currencies,
}

impl Osmosis {
    async fn query_supported_currencies(
        &self,
        node_rpc: TonicChannel,
    ) -> Result<impl Iterator<Item = Route> + '_, query::error::Wasm> {
        query::wasm_smart::<SupportedCurrencyPairsResponse>(
            &mut WasmQueryClient::new(node_rpc),
            self.oracle_addr.to_string(),
            QueryMsg::SUPPORTED_CURRENCY_PAIRS.to_vec(),
        )
        .await
        .map(|swap_legs: Vec<SwapLeg>| {
            swap_legs
                .into_iter()
                .filter_map(|swap: SwapLeg| -> Option<Route> {
                    let (from_symbol, from_decimal_places): (
                        Arc<SymbolUnsized>,
                        u8,
                    ) = self.currencies.get(&swap.from).map(
                        |symbol_and_decimal_places: &SymbolAndDecimalPlaces| {
                            (
                                symbol_and_decimal_places.denom().clone(),
                                symbol_and_decimal_places.decimal_places(),
                            )
                        },
                    )?;

                    let (to_symbol, to_decimal_places): (
                        Arc<SymbolUnsized>,
                        u8,
                    ) = self.currencies.get(&swap.to.target).map(
                        |symbol_and_decimal_places: &SymbolAndDecimalPlaces| {
                            (
                                symbol_and_decimal_places.denom().clone(),
                                symbol_and_decimal_places.decimal_places(),
                            )
                        },
                    )?;

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
        const DECIMAL_PLACES_IN_RESPONSE: usize = 36;
        const MAX_U128_DECIMAL_DIGITS: usize = 38;

        let mut set: JoinSet<
            Result<Price<CoinWithDecimalPlaces>, ProviderError>,
        > = JoinSet::new();

        let routes_iter = self
            .query_supported_currencies(self.node_client.raw_grpc())
            .await?;

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
        } in routes_iter
        {
            let channel = self.channel.clone();

            set.spawn(async move {
                query::raw(
                    channel,
                    SpotPriceRequest {
                        pool_id,
                        base_asset_denom: from_symbol.to_string(),
                        quote_asset_denom: to_symbol.to_string(),
                    },
                    "/osmosis.poolmanager.v2.Query/SpotPriceV2",
                )
                .await
                .map_err(|error| {
                    ProviderError::WasmQuery(
                        format!("currency pair: {from_ticker}/{to_ticker}"),
                        query::error::Wasm::RawQuery(error),
                    )
                })
                .and_then(|SpotPriceResponse { mut spot_price }| {
                    debug!(
                        r#"Osmosis returned "{spot_price}" for the pair {from_ticker}/{to_ticker} from pool #{pool_id}."#
                    );

                    if spot_price.is_ascii() {
                        Ok(
                            if let Some(zeroes_needed) =
                                DECIMAL_PLACES_IN_RESPONSE.checked_sub(spot_price.len())
                            {
                                String::from(".")
                                    + &String::from('0').repeat(zeroes_needed)
                                    + &spot_price
                            } else {
                                spot_price
                                    .insert(spot_price.len() - DECIMAL_PLACES_IN_RESPONSE, '.');

                                spot_price
                            },
                        )
                    } else {
                        Err(ProviderError::NonAsciiResponse(format!(
                            "currency pair: {from_ticker}/{to_ticker}",
                        )))
                    }
                })
                .and_then(|spot_price| {
                    spot_price[..spot_price
                        .len()
                        .min(MAX_U128_DECIMAL_DIGITS + 1 /* Added dot */)]
                        .try_into()
                        .map_err(|error| {
                            ProviderError::ParsePrice(
                                format!("currency pair: {from_ticker}/{to_ticker}"),
                                error,
                            )
                        })
                        .map(|ratio: Ratio| {
                            ratio.as_quote_to_price_with_decimal_places(
                                from_ticker,
                                from_decimal_places,
                                to_ticker,
                                to_decimal_places,
                            )
                        })
                })
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
        node_client: &NodeClient,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<false>,
    {
        let currencies = config
            .misc_mut()
            .remove("currencies")
            .ok_or(ConstructError::MissingField("currencies"))
            .and_then(|value: Value| {
                value.try_into().map_err(|error: toml::de::Error| {
                    ConstructError::DeserializeField("currencies", error)
                })
            })?;

        let oracle_addr: Arc<str> = config.oracle_addr().clone();

        if let Some(fields) = super::left_over_fields(config.into_misc()) {
            Err(ConstructError::UnknownFields(fields))
        } else {
            let grpc_uri = Config::fetch_from_env(id, "GRPC_URI")
                .map_err(ConstructError::FetchGrpcUri)?;

            NodeClient::new(&grpc_uri, None)
                .await
                .map(|osmosis_client| Self {
                    instance_id: id.to_string(),
                    node_client: node_client.clone(),
                    channel: osmosis_client.raw_grpc(),
                    oracle_addr,
                    currencies,
                })
                .map_err(From::from)
        }
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
    #[error("Failed to fetch Osmosis node's gRPC URI from environment variables! Cause: {0}")]
    FetchGrpcUri(#[from] EnvError),
    #[error("Failed to connect gRPC endpoint! Cause: {0}")]
    ConnectToGrpc(#[from] client::error::Error),
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
