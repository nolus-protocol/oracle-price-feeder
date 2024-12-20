use std::{collections::BTreeMap, future::Future};

use anyhow::{Context as _, Result};

use chain_ops::node;

use crate::{
    oracle::Oracle,
    provider::{Amount, Base, CurrencyPair, Decimal, Provider, Quote},
};

use super::{
    AssetInfo, Astroport, QueryMsg, SimulateSwapOperationsResponse,
    SwapOperation,
};

impl Astroport {
    pub const fn new(router_address: String) -> Self {
        Self { router_address }
    }

    fn price_query_message(
        base: String,
        base_decimal_places: u8,
        quote: String,
        quote_decimal_places: u8,
    ) -> Result<<Self as Provider>::PriceQueryMessage> {
        let base_amount = 10_u128.pow(base_decimal_places.into());

        serde_json_wasm::to_vec(&QueryMsg::SimulateSwapOperations {
            offer_amount: base_amount.to_string(),
            operations: [SwapOperation::AstroSwap {
                offer_asset_info: AssetInfo::NativeToken { denom: base },
                ask_asset_info: AssetInfo::NativeToken { denom: quote },
            }],
        })
        .map(|message| PriceQueryMessage {
            base_amount: Amount::new(Decimal::new(
                base_amount.to_string(),
                base_decimal_places,
            )),
            quote_decimal_places,
            message,
        })
        .context("Failed to serialize price query message!")
    }
}

impl Provider for Astroport {
    type PriceQueryMessage = PriceQueryMessage;

    const PROVIDER_NAME: &'static str = "Astroport";

    fn price_query_messages(
        &self,
        oracle: &Oracle,
    ) -> Result<BTreeMap<CurrencyPair, Self::PriceQueryMessage>> {
        let currencies = oracle.currencies();

        oracle
            .currency_pairs()
            .keys()
            .map(|(base, quote)| {
                let base_currency = currencies.get(base)?;
                let quote_currency = currencies.get(quote)?;

                Self::price_query_message(
                    base_currency.dex_symbol.clone(),
                    base_currency.decimal_digits,
                    quote_currency.dex_symbol.clone(),
                    quote_currency.decimal_digits,
                )
                .with_context(|| {
                    format!(
                        "Failed to construct price query message! \
                    Currency pair={base}/{quote}"
                    )
                })
                .map(|query_message| {
                    (
                        CurrencyPair {
                            base: base.clone().into(),
                            quote: quote.clone().into(),
                        },
                        query_message,
                    )
                })
            })
            .collect()
    }

    fn price_query(
        &self,
        dex_node_client: &node::Client,
        &PriceQueryMessage {
            ref base_amount,
            quote_decimal_places,
            ref message,
        }: &Self::PriceQueryMessage,
    ) -> impl Future<Output = Result<(Amount<Base>, Amount<Quote>)>> + Send + 'static
    {
        let mut query_wasm = dex_node_client.clone().query_wasm();

        let router_address = self.router_address.clone();

        let base_amount = base_amount.clone();

        let message = message.clone();

        async move {
            query_wasm
                .smart(router_address, message)
                .await
                .map(|SimulateSwapOperationsResponse { amount }| {
                    (
                        base_amount,
                        Amount::new(Decimal::new(amount, quote_decimal_places)),
                    )
                })
                .context("Failed to query price from router contract!")
        }
    }
}

pub struct PriceQueryMessage {
    base_amount: Amount<Base>,
    quote_decimal_places: u8,
    message: Vec<u8>,
}
