use std::{convert::identity, time::Duration};

use anyhow::{Context as _, Result};
use cosmrs::{
    proto::{
        cosmos::base::abci::v1beta1::TxResponse,
        cosmwasm::wasm::v1::{MsgExecuteContract, MsgExecuteContractResponse},
    },
    tendermint::abci::Code as TxCode,
    tx::Body as TxBody,
    Any, Gas,
};
use data_encoding::HEXUPPER;
use prost::Message;
use serde::{de::DeserializeOwned, Serialize};
use tokio::time::{sleep, timeout};

use crate::node;

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "fetch-tx",
            $($body)+
        );
    };
}

pub const OUT_OF_GAS_ERROR_CODE: u32 = 11;

pub struct ExecuteTemplate(MsgExecuteContract);

impl ExecuteTemplate {
    #[must_use]
    pub const fn new(signer_address: String, contract_address: String) -> Self {
        Self(MsgExecuteContract {
            sender: signer_address,
            contract: contract_address,
            msg: vec![],
            funds: vec![],
        })
    }

    pub fn apply<M: Serialize + ?Sized>(
        &mut self,
        message: &M,
    ) -> Result<TxBody> {
        serde_json_wasm::to_vec(message)
            .context("Failed to serialize message into JSON format!")
            .and_then(|message| self.apply_raw(message))
    }

    pub fn apply_raw(&mut self, message: Vec<u8>) -> Result<TxBody> {
        self.0.msg = message;

        let result = Any::from_msg(&self.0)
            .map(|message| TxBody {
                messages: vec![message],
                memo: String::new(),
                timeout_height: 0_u32.into(),
                extension_options: vec![],
                non_critical_extension_options: vec![],
            })
            .context("Failed to encode message into binary Protobuf format!");

        self.0.msg = vec![];

        result
    }
}

pub async fn fetch_delivered(
    query_tx: &mut node::QueryTx,
    source: &str,
    TxResponse {
        code,
        txhash: hash,
        raw_log: log,
        ..
    }: TxResponse,
    timeout_duration: Duration,
) -> Result<Option<TxResponse>> {
    const PRINT_ON_NTH: u8 = 5;
    const IDLE_SLEEP_DURATION: Duration = Duration::from_secs(2);

    if TxCode::from(code).is_ok() {
        timeout(timeout_duration * PRINT_ON_NTH.into(), async move {
            let mut not_included_counter = 0;

            loop {
                match query_tx.tx(hash.clone()).await {
                    Ok(Some(response)) => break Ok(response),
                    Ok(None) => {
                        not_included_counter =
                            (not_included_counter + 1) % PRINT_ON_NTH;

                        if not_included_counter == 0 {
                            log!(info!(
                                %source,
                                %hash,
                                "Transaction not included in block.",
                            ));
                        }
                    },
                    Err(error) => {
                        log!(error!(
                            %source,
                            %hash,
                            ?error,
                            "Error occurred while fetching processed \
                            transaction!",
                        ));
                    },
                }

                sleep(IDLE_SLEEP_DURATION).await;
            }
        })
        .await
        .context("Timed out while fetching processed transaction!")
        .and_then(identity)
        .map(Some)
    } else {
        log!(error!(
            %hash,
            ?log,
            "Transaction failed upon broadcast!",
        ));

        Ok(None)
    }
}

pub fn adjust_fallback_gas(fallback_gas: Gas, gas_used: Gas) -> Result<Gas> {
    const FALLBACK_GAS_MAJOR_COEFFICIENT: u128 = 2;
    const FALLBACK_GAS_MINOR_COEFFICIENT: u128 = 1;
    const FALLBACK_GAS_DENOMINATOR: u128 =
        FALLBACK_GAS_MAJOR_COEFFICIENT + FALLBACK_GAS_MINOR_COEFFICIENT;

    let (fallback_gas_major, fallback_gas_minor) = {
        if gas_used < fallback_gas {
            (fallback_gas, gas_used)
        } else {
            (gas_used, fallback_gas)
        }
    };

    (((u128::from(fallback_gas_major) * FALLBACK_GAS_MAJOR_COEFFICIENT)
        + (u128::from(fallback_gas_minor) * FALLBACK_GAS_MINOR_COEFFICIENT))
        / FALLBACK_GAS_DENOMINATOR)
        .try_into()
        .map_err(Into::into)
}

pub fn decode_execute_response<T>(tx_response: &TxResponse) -> Result<T>
where
    T: DeserializeOwned,
{
    HEXUPPER
        .decode(tx_response.data.as_bytes())
        .context("Transaction response payload should be BASE64 encoded!")
        .and_then(|protobuf| {
            Package::decode(protobuf.as_slice()).map_err(Into::into)
        })
        .and_then(|protobuf| {
            Any::decode(protobuf.data.as_slice()).map_err(Into::into)
        })
        .and_then(|response| response.to_msg().map_err(Into::into))
        .map(|response: MsgExecuteContractResponse| response.data)
        .and_then(|response| {
            serde_json_wasm::from_slice(&response).map_err(Into::into)
        })
}

#[derive(Message)]
struct Package {
    #[prost(bytes, tag = "2")]
    data: Vec<u8>,
}
