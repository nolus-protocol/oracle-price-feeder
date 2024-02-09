use std::{collections::BTreeMap, sync::Arc};

use astroport::{
    asset::AssetInfo,
    router::{QueryMsg, SwapOperation},
};
use async_trait::async_trait;
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::{debug, error};

use chain_comms::{
    client::Client as NodeClient,
    interact::query,
    reexport::{
        cosmrs::proto::cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient,
        tonic::{
            codegen::http::uri::InvalidUri,
            transport::{Channel as TonicChannel, Error as TonicError, Uri},
        },
    },
};

use crate::{
    config::{Currencies, EnvError, ProviderConfigExt, SymbolAndDecimalPlaces, SymbolUnsized},
    messages::{QueryMsg as OracleQueryMsg, SupportedCurrencyPairsResponse, SwapLeg},
    price::{CoinWithDecimalPlaces, Price},
    provider::{FromConfig, Provider, ProviderError},
};

pub(super) struct Astroport {
    instance_id: String,
    node_wasm_query_client: WasmQueryClient<TonicChannel>,
    oracle_addr: Arc<str>,
    wasm_query_client: WasmQueryClient<TonicChannel>,
    router_contract: Arc<str>,
    currencies: Currencies,
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

    async fn supported_currencies_intersection(
        &self,
    ) -> Result<
        impl Iterator<Item = (String, Arc<str>, u8, String, Arc<str>, u8)> + '_,
        ProviderError,
    > {
        query::wasm_smart::<SupportedCurrencyPairsResponse>(
            &mut self.node_wasm_query_client.clone(),
            self.oracle_addr.to_string(),
            OracleQueryMsg::SUPPORTED_CURRENCY_PAIRS,
        )
        .await
        .map(|supported_currencies: SupportedCurrencyPairsResponse| {
            supported_currencies
                .into_iter()
                .filter_map(|swap_leg: SwapLeg| {
                    self.currencies
                        .get(&swap_leg.from)
                        .and_then(|base: &SymbolAndDecimalPlaces| {
                            self.currencies.get(&swap_leg.to.target).map(
                                |quote: &SymbolAndDecimalPlaces| {
                                    (
                                        swap_leg.from,
                                        base.denom().clone(),
                                        base.decimal_places(),
                                        swap_leg.to.target,
                                        quote.denom().clone(),
                                        quote.decimal_places(),
                                    )
                                },
                            )
                        })
                })
        })
        .map_err(From::from)
    }
}

#[async_trait]
impl Provider for Astroport {
    fn instance_id(&self) -> &str {
        &self.instance_id
    }

    async fn get_prices(
        &self,
        fault_tolerant: bool,
    ) -> Result<Box<[Price<CoinWithDecimalPlaces>]>, ProviderError> {
        let mut set: JoinSet<Result<Price<CoinWithDecimalPlaces>, ProviderError>> = JoinSet::new();

        let supported_currencies_iter = self.supported_currencies_intersection().await?;

        for (
            base_ticker,
            base_dex_denom,
            base_decimal_places,
            quote_ticker,
            quote_dex_denom,
            quote_decimal_places,
        ) in supported_currencies_iter
        {
            let mut wasm_query_client: WasmQueryClient<TonicChannel> =
                self.wasm_query_client.clone();

            let router_contract: Arc<str> = self.router_contract.clone();

            let max_decimal_places: u8 = base_decimal_places.max(quote_decimal_places);

            set.spawn(async move {
                let query_message =
                    Self::query_message(&base_dex_denom, &quote_dex_denom, max_decimal_places)?;

                debug!(query_message = %String::from_utf8_lossy(&query_message), "Query message");

                let query_result: Result<
                    astroport::router::SimulateSwapOperationsResponse,
                    query::error::Wasm,
                > = query::wasm_smart(&mut wasm_query_client, router_contract.to_string(), &{
                    query_message
                })
                .await;

                match query_result {
                    Ok(astroport::router::SimulateSwapOperationsResponse {
                        amount: quote_amount,
                    }) => Ok(Price::new(
                        CoinWithDecimalPlaces::new(
                            10_u128.pow(max_decimal_places.into()),
                            base_ticker,
                            base_decimal_places,
                        ),
                        CoinWithDecimalPlaces::new(
                            quote_amount.u128(),
                            quote_ticker,
                            quote_decimal_places,
                        ),
                    )),
                    Err(error) => Err(ProviderError::WasmQuery(
                        format!(r#"currency pair = "{base_ticker}/{quote_ticker}""#),
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
        const CURRENCIES_FIELD: &str = "currencies";

        let grpc_uri: Uri = Config::fetch_from_env(id, GRPC_URI_ENV_NAME)
            .map_err(ConstructError::FetchGrpcUri)
            .and_then(|value: String| value.parse().map_err(ConstructError::InvalidGrpcUri))?;

        let router_contract: Arc<str> = Config::fetch_from_env(id, ROUTER_CONTRACT_ENV_NAME)
            .map_err(ConstructError::FetchRouterContract)
            .map(Into::into)?;

        let oracle_addr: Arc<str> = config.oracle_addr().clone();

        let mut config: BTreeMap<String, toml::Value> = config.into_misc();

        let currencies: Currencies = config
            .remove(CURRENCIES_FIELD)
            .ok_or(ConstructError::MissingField(CURRENCIES_FIELD))?
            .try_into()
            .map_err(|error: toml::de::Error| {
                ConstructError::DeserializeField(CURRENCIES_FIELD, error)
            })?;

        if let Some(fields) = super::left_over_fields(config) {
            Err(ConstructError::UnknownFields(fields))
        } else {
            Ok(Self {
                instance_id: id.to_string(),
                node_wasm_query_client: node_client.wasm_query_client(),
                oracle_addr,
                wasm_query_client: TonicChannel::builder(grpc_uri)
                    .connect()
                    .await
                    .map(WasmQueryClient::new)?,
                router_contract,
                currencies,
            })
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
    #[error("Failed to fetch gRPC's URI from environment variables! Cause: {0}")]
    FetchGrpcUri(EnvError),
    #[error("Failed to parse gRPC's URI! Cause: {0}")]
    InvalidGrpcUri(#[from] InvalidUri),
    #[error("Failed to fetch router contract's address from environment variables! Cause: {0}")]
    FetchRouterContract(EnvError),
    #[error("Failed to connect RPC's URI! Cause: {0}")]
    ConnectToGrpc(#[from] TonicError),
}
