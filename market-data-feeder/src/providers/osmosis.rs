use std::sync::Arc;

use async_trait::async_trait;
use osmosis_std::types::osmosis::poolmanager::v2::{
    SpotPriceRequest, SpotPriceResponse,
};
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::debug;

use chain_comms::{
    client::{self, Client as NodeClient},
    interact::{healthcheck::Healthcheck, query},
    reexport::{
        cosmrs::proto::cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient,
        tonic::transport::Channel as TonicChannel,
    },
};

use crate::{
    config::{EnvError, ProviderConfigExt},
    messages::PoolId,
    oracle::{query_currencies, query_supported_currencies},
    price::{CoinWithDecimalPlaces, Price, Ratio},
    provider::{FromConfig, Provider, ProviderError},
};

use super::TickerSymbolDecimalPlaces;

pub(crate) struct Osmosis {
    instance_id: String,
    node_wasm_query_client: WasmQueryClient<TonicChannel>,
    oracle_address: Arc<str>,
    channel: TonicChannel,
    healthcheck: Healthcheck,
}

impl Osmosis {
    async fn query_supported_currencies(
        &mut self,
    ) -> Result<impl Iterator<Item = Route>, query::error::Wasm> {
        let oracle_address = self.oracle_address.to_string();

        let swap_legs = query_supported_currencies(
            &mut self.node_wasm_query_client,
            oracle_address.clone(),
        )
        .await?;

        query_currencies(&mut self.node_wasm_query_client, oracle_address)
            .await
            .map(|currencies| {
                swap_legs.into_iter().filter_map(move |swap| {
                    currencies
                        .get(&swap.from)
                        .zip(currencies.get(&swap.to.target))
                        .map(|(from, to)| Route {
                            pool_id: swap.to.pool_id,
                            from: TickerSymbolDecimalPlaces {
                                ticker: swap.from,
                                symbol: from.dex_symbol.clone(),
                                decimal_places: from.decimal_digits,
                            },
                            to: TickerSymbolDecimalPlaces {
                                ticker: swap.to.target,
                                symbol: to.dex_symbol.clone(),
                                decimal_places: to.decimal_digits,
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

    fn healthcheck(&mut self) -> &mut Healthcheck {
        &mut self.healthcheck
    }

    async fn get_prices(
        &mut self,
        fault_tolerant: bool,
    ) -> Result<Box<[Price<CoinWithDecimalPlaces>]>, ProviderError> {
        const DECIMAL_PLACES_IN_RESPONSE: usize = 36;
        const MAX_U128_DECIMAL_DIGITS: usize = 38;

        let mut set: JoinSet<
            Result<Price<CoinWithDecimalPlaces>, ProviderError>,
        > = JoinSet::new();

        let routes_iter =
            self.query_supported_currencies().await.map_err(|error| {
                ProviderError::WasmQuery(self.instance_id.clone(), error)
            })?;

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
        config: Config,
        node_client: &NodeClient,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<false>,
    {
        let oracle_address: Arc<str> = config.oracle_addr().clone();

        if let Some(fields) = super::left_over_fields(config.into_misc()) {
            Err(ConstructError::UnknownFields(fields))
        } else {
            let grpc_uri = Config::fetch_from_env(id, "GRPC_URI")
                .map_err(ConstructError::FetchGrpcUri)?;

            let osmosis_client = NodeClient::new(&grpc_uri, None).await?;

            Healthcheck::new(osmosis_client.tendermint_service_client())
                .await
                .map(|healthcheck| Self {
                    instance_id: id.to_string(),
                    node_wasm_query_client: node_client.wasm_query_client(),
                    oracle_address,
                    channel: osmosis_client.raw_grpc(),
                    healthcheck,
                })
                .map_err(From::from)
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum ConstructError {
    #[error("Unknown fields found! Unknown fields: {0}")]
    UnknownFields(Box<str>),
    #[error("Failed to fetch Osmosis node's gRPC URI from environment variables! Cause: {0}")]
    FetchGrpcUri(#[from] EnvError),
    #[error("Failed to construct node communication client! Cause: {0}")]
    ConstructNodeClient(#[from] client::error::Error),
    #[error("Failed to connect gRPC endpoint! Cause: {0}")]
    Healthcheck(#[from] chain_comms::interact::healthcheck::error::Construct),
}

struct Route {
    pool_id: PoolId,
    from: TickerSymbolDecimalPlaces,
    to: TickerSymbolDecimalPlaces,
}
