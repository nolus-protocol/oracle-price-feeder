use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use http::Uri;
use serde::{
    de::{Deserializer, Error as DeserializeError},
    Deserialize, Serialize,
};
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::error;

use chain_comms::{
    client::Client as NodeClient,
    interact::query_wasm,
    reexport::tonic::transport::{Channel as TonicChannel, Error as TonicError},
};

use crate::{
    config::{Currencies, EnvError, ProviderConfigExt, SymbolAndDecimalPlaces, SymbolUnsized},
    messages::{QueryMsg as OracleQueryMsg, SupportedCurrencyPairsResponse, SwapLeg},
    price::{CoinWithDecimalPlaces, Price},
    provider::{FromConfig, Provider, ProviderError},
};

pub(super) struct Astroport {
    instance_id: String,
    node_client: NodeClient,
    oracle_addr: Arc<str>,
    channel: TonicChannel,
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
            offer_amount: 10_u128.pow(decimal_places.into()).to_string(),
            operations: Vec::from([SwapOperation::AstroSwap {
                offer_asset_info: AssetInfo::NativeToken { denom: base },
                ask_asset_info: AssetInfo::NativeToken { denom: quote },
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
        self.node_client
            .with_grpc(|rpc: TonicChannel| {
                query_wasm::<SupportedCurrencyPairsResponse>(
                    rpc,
                    self.oracle_addr.to_string(),
                    OracleQueryMsg::SUPPORTED_CURRENCY_PAIRS,
                )
            })
            .await
            .map(|supported_currencies: SupportedCurrencyPairsResponse| {
                supported_currencies
                    .into_iter()
                    .filter_map(|swap_leg: SwapLeg| {
                        self.currencies.get(&swap_leg.from).and_then(
                            |base: &SymbolAndDecimalPlaces| {
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
                            },
                        )
                    })
            })
            .map_err(ProviderError::WasmQuery)
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

        for (
            base_ticker,
            base_dex_denom,
            base_decimal_places,
            quote_ticker,
            quote_dex_denom,
            quote_decimal_places,
        ) in self.supported_currencies_intersection().await?
        {
            let channel: TonicChannel = self.channel.clone();
            let router_contract: Arc<str> = self.router_contract.clone();
            let max_decimal_places: u8 = base_decimal_places.max(quote_decimal_places);

            set.spawn(async move {
                query_wasm(
                    channel,
                    router_contract.to_string(),
                    &Self::query_message(&base_dex_denom, &quote_dex_denom, max_decimal_places)?,
                )
                .await
                .map(
                    |SimulateSwapOperationsResponse {
                         amount: quote_amount,
                     }: SimulateSwapOperationsResponse| {
                        Price::new(
                            CoinWithDecimalPlaces::new(
                                10_u128.pow(max_decimal_places.into()),
                                base_ticker,
                                base_decimal_places,
                            ),
                            CoinWithDecimalPlaces::new(
                                quote_amount,
                                quote_ticker,
                                quote_decimal_places,
                            ),
                        )
                    },
                )
                .map_err(ProviderError::WasmQuery)
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

        let grpc_url: Uri = Config::fetch_from_env(id, GRPC_URI_ENV_NAME)
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

        if let Some(fields) = config
            .into_keys()
            .reduce(|mut accumulator: String, field: String| {
                accumulator.reserve(field.len() + 2);

                accumulator.push_str(", ");

                accumulator.push_str(&field);

                accumulator
            })
            .map(String::into_boxed_str)
        {
            Err(ConstructError::UnknownFields(fields))
        } else {
            Ok(Self {
                instance_id: id.to_string(),
                node_client: node_client.clone(),
                oracle_addr,
                channel: TonicChannel::builder(grpc_url).connect().await?,
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
    InvalidGrpcUri(#[from] http::uri::InvalidUri),
    #[error("Failed to fetch router contract's address from environment variables! Cause: {0}")]
    FetchRouterContract(EnvError),
    #[error("Failed to connect RPC's URI! Cause: {0}")]
    ConnectToGrpc(#[from] TonicError),
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg<'r> {
    SimulateSwapOperations {
        offer_amount: String,
        operations: Vec<SwapOperation<'r>>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapOperation<'r> {
    AstroSwap {
        offer_asset_info: AssetInfo<'r>,
        ask_asset_info: AssetInfo<'r>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetInfo<'r> {
    NativeToken { denom: &'r SymbolUnsized },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SimulateSwapOperationsResponse {
    #[serde(deserialize_with = "deserialize_str_as_u128")]
    pub amount: u128,
}

fn deserialize_str_as_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    <&'de str>::deserialize(deserializer)
        .and_then(|value: &'de str| value.parse().map_err(DeserializeError::custom))
}
