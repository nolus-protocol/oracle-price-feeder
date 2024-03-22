use cosmrs::{
    proto::cosmos::{
        base::abci::v1beta1::TxResponse,
        tx::v1beta1::{GetTxRequest, GetTxResponse},
    },
    tendermint::abci::Code as TxCode,
};
use tonic::Response as TonicResponse;

use crate::client::Client;

use self::error::Error;

pub mod error;

pub struct Response {
    pub code: TxCode,
    pub block_height: u64,
    pub data: Box<str>,
    pub raw_log: Box<str>,
    pub info: Box<str>,
    pub gas_wanted: u64,
    pub gas_used: u64,
}

pub async fn get_tx_response(
    client: &Client,
    tx_hash: String,
) -> Result<Response, Error> {
    client
        .tx_service_client()
        .get_tx(GetTxRequest { hash: tx_hash })
        .await
        .map(TonicResponse::into_inner)
        .map_err(Error::Rpc)
        .and_then(|GetTxResponse { tx_response, .. }| {
            tx_response.ok_or(Error::EmptyResponseReceived)
        })
        .map(
            |TxResponse {
                 height: block_height,
                 code,
                 data,
                 raw_log,
                 info,
                 gas_wanted,
                 gas_used,
                 ..
             }| Response {
                code: code.into(),
                block_height: block_height.unsigned_abs(),
                data: data.into_boxed_str(),
                raw_log: raw_log.into_boxed_str(),
                info: info.into_boxed_str(),
                gas_wanted: gas_wanted.unsigned_abs(),
                gas_used: gas_used.unsigned_abs(),
            },
        )
}
