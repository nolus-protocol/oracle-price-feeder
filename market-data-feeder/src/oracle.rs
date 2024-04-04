use std::{collections::BTreeMap, sync::Arc};

use serde::Deserialize;

use chain_comms::{
    interact::query,
    reexport::{
        cosmrs::proto::cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient,
        tonic::transport::Channel as TonicChannel,
    },
};

use crate::messages::{QueryMsg, SupportedCurrencyPairsResponse};

pub(crate) type TickerUnsized = str;
pub(crate) type Ticker = String;

pub(crate) type SymbolUnsized = str;

pub(crate) type Currencies = BTreeMap<Ticker, SymbolAndDecimalPlaces>;

#[derive(Debug, Clone)]
#[must_use]
pub(crate) struct SymbolAndDecimalPlaces {
    pub dex_symbol: Arc<SymbolUnsized>,
    pub decimal_digits: u8,
}

pub(crate) async fn query_supported_currencies(
    wasm_query_client: &mut WasmQueryClient<TonicChannel>,
    oracle_address: String,
) -> Result<SupportedCurrencyPairsResponse, query::error::Wasm> {
    query::wasm_smart::<SupportedCurrencyPairsResponse>(
        wasm_query_client,
        oracle_address,
        QueryMsg::SUPPORTED_CURRENCY_PAIRS.to_vec(),
    )
    .await
}

pub(crate) async fn query_currencies(
    wasm_query_client: &mut WasmQueryClient<TonicChannel>,
    oracle_address: String,
) -> Result<Currencies, query::error::Wasm> {
    query::wasm_smart::<CurrenciesResponse>(
        wasm_query_client,
        oracle_address,
        QueryMsg::CURRENCIES.to_vec(),
    )
    .await
    .map(|response| {
        response
            .into_iter()
            .map(
                |Currency {
                     ticker,
                     dex_symbol,
                     decimal_digits,
                     ..
                 }| {
                    (
                        ticker,
                        SymbolAndDecimalPlaces {
                            dex_symbol: Arc::from(dex_symbol.into_boxed_str()),
                            decimal_digits,
                        },
                    )
                },
            )
            .collect::<Currencies>()
    })
}

type CurrenciesResponse = Vec<Currency>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Currency {
    ticker: String,
    dex_symbol: String,
    decimal_digits: u8,
}
