use std::sync::Arc;

use astroport::{
    asset::AssetInfo,
    router::{QueryMsg, SwapOperation},
};
use async_trait::async_trait;
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::{debug, error};

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
    oracle::{
        query_currencies, query_supported_currencies, SymbolAndDecimalPlaces,
        SymbolUnsized,
    },
    price::{CoinWithDecimalPlaces, Price},
    provider::{FromConfig, Provider, ProviderError},
};

pub(super) struct Astroport {
    instance_id: String,
    node_wasm_query_client: WasmQueryClient<TonicChannel>,
    oracle_address: Arc<str>,
    dex_wasm_query_client: WasmQueryClient<TonicChannel>,
    router_contract: Arc<str>,
    healthcheck: Healthcheck,
}

impl Astroport {
    fn query_message(
        base: &SymbolUnsized,
        quote: &SymbolUnsized,
        decimal_places: u8,
    ) -> Result<Vec<u8>, ProviderError> {
        serde_json_wasm::to_vec(&QueryMsg::SimulateSwapOperations {
            offer_amount: 10_u128.pow(decimal_places.into()).into(),
            operations: Vec::from([SwapOperation::AstroSwap {
                offer_asset_info: AssetInfo::NativeToken { denom: base.into() },
                ask_asset_info: AssetInfo::NativeToken {
                    denom: quote.into(),
                },
            }]),
        })
        .map_err(Into::into)
    }

    async fn query_supported_currencies(
        &mut self,
    ) -> Result<
        impl Iterator<
            Item = (
                (String, SymbolAndDecimalPlaces),
                (String, SymbolAndDecimalPlaces),
            ),
        >,
        query::error::Wasm,
    > {
        let oracle_address = self.oracle_address.to_string();

        let swap_legs = query_supported_currencies(
            &mut self.node_wasm_query_client,
            oracle_address.clone(),
        )
        .await?;

        query_currencies(&mut self.node_wasm_query_client, oracle_address)
            .await
            .map(|currencies| {
                swap_legs.into_iter().filter_map(move |swap_leg| {
                    currencies
                        .get(&swap_leg.from)
                        .zip(currencies.get(&swap_leg.to.target))
                        .map(|(from, to)| {
                            (
                                (swap_leg.from, from.clone()),
                                (swap_leg.to.target, to.clone()),
                            )
                        })
                })
            })
    }
}

#[async_trait]
impl Provider for Astroport {
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
        let mut set: JoinSet<
            Result<Price<CoinWithDecimalPlaces>, ProviderError>,
        > = JoinSet::new();

        let supported_currencies_iter =
            self.query_supported_currencies().await.map_err(|error| {
                ProviderError::WasmQuery(self.instance_id.clone(), error)
            })?;

        for ((from_ticker, from_currency), (to_ticker, to_currency)) in
            supported_currencies_iter
        {
            let mut astroport_wasm_query_client: WasmQueryClient<TonicChannel> =
                self.dex_wasm_query_client.clone();

            let router_contract: Arc<str> = self.router_contract.clone();

            let max_decimal_places: u8 =
                from_currency.decimal_digits.max(to_currency.decimal_digits);

            set.spawn(async move {
                let query_message =
                    Self::query_message(&from_currency.dex_symbol, &to_currency.dex_symbol, max_decimal_places)?;

                debug!(query_message = %String::from_utf8_lossy(&query_message), "Query message");

                let query_result: Result<
                    astroport::router::SimulateSwapOperationsResponse,
                    query::error::Wasm,
                > = query::wasm_smart(
                    &mut astroport_wasm_query_client,
                    router_contract.to_string(),
                    query_message,
                )
                    .await;

                match query_result {
                    Ok(astroport::router::SimulateSwapOperationsResponse {
                           amount: quote_amount,
                       }) => Ok(Price::new(
                        CoinWithDecimalPlaces::new(
                            10_u128.pow(max_decimal_places.into()),
                            from_ticker,
                            from_currency.decimal_digits,
                        ),
                        CoinWithDecimalPlaces::new(
                            quote_amount.u128(),
                            to_ticker,
                            to_currency.decimal_digits,
                        ),
                    )),
                    Err(error) => Err(ProviderError::WasmQuery(
                        format!(r#"currency pair = "{from_ticker}/{to_ticker}""#),
                        error,
                    )),
                }
            });
        }

        super::collect_prices_from_task_set(set, fault_tolerant).await
    }
}

#[async_trait]
impl FromConfig<false> for Astroport {
    const ID: &'static str = "astroport";

    type ConstructError = ConstructError;

    async fn from_config<Config>(
        id: &str,
        config: Config,
        node_client: &NodeClient,
    ) -> Result<Self, Self::ConstructError>
    where
        Config: ProviderConfigExt<false>,
    {
        const GRPC_URI_ENV_NAME: &str = "grpc_uri";
        const ROUTER_CONTRACT_ENV_NAME: &str = "router_addr";

        let grpc_uri = Config::fetch_from_env(id, GRPC_URI_ENV_NAME)
            .map_err(ConstructError::FetchGrpcUri)?;

        let router_contract: Arc<str> =
            Config::fetch_from_env(id, ROUTER_CONTRACT_ENV_NAME)
                .map_err(ConstructError::FetchRouterContract)
                .map(Into::into)?;

        let oracle_addr: Arc<str> = config.oracle_addr().clone();

        if let Some(fields) = super::left_over_fields(config.into_misc()) {
            Err(ConstructError::UnknownFields(fields))
        } else {
            let dex_client = NodeClient::new(&grpc_uri, None).await?;

            Healthcheck::new(dex_client.tendermint_service_client())
                .await
                .map(|healthcheck| Self {
                    instance_id: id.to_string(),
                    node_wasm_query_client: node_client.wasm_query_client(),
                    oracle_address: oracle_addr,
                    dex_wasm_query_client: dex_client.wasm_query_client(),
                    router_contract,
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
    #[error(
        "Failed to fetch gRPC's URI from environment variables! Cause: {0}"
    )]
    FetchGrpcUri(EnvError),
    #[error("Failed to fetch router contract's address from environment variables! Cause: {0}")]
    FetchRouterContract(EnvError),
    #[error("Failed to construct node communication client! Cause: {0}")]
    ConstructNodeClient(#[from] client::error::Error),
    #[error("Failed to connect gRPC endpoint! Cause: {0}")]
    Healthcheck(#[from] chain_comms::interact::healthcheck::error::Construct),
}
