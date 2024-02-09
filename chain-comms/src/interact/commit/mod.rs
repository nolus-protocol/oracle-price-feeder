use std::num::NonZeroU64;

use cosmrs::{
    proto::{
        cosmos::{
            base::abci::v1beta1::TxResponse,
            tx::v1beta1::{BroadcastMode, BroadcastTxRequest, BroadcastTxResponse, TxRaw as RawTx},
        },
        prost::Message as _,
    },
    tendermint::abci::Code as TxCode,
    tx::Body as TxBody,
    Any as ProtobufAny,
};
use tonic::Response as TonicResponse;

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

use super::{
    adjust_gas_limit, calculate_fee, process_simulation_result, simulate, simulate::simulate,
    TxHash,
};

use self::error::{CommitTx as Error, GasEstimatingTxCommit as ErrorWithEstimation};

pub mod error;

pub struct Response {
    pub code: TxCode,
    pub raw_log: Box<str>,
    pub info: Box<str>,
    pub tx_hash: TxHash,
}

#[allow(clippy::future_not_send)]
pub async fn commit(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: NonZeroU64,
    unsigned_tx: ContractTx,
) -> Result<Response, Error> {
    let signed_tx = unsigned_tx
        .commit(signer, calculate_fee(node_config, gas_limit), None, None)
        .map(|signed_tx: RawTx| signed_tx.encode_to_vec())?;

    with_signed_body(client, signed_tx, signer).await
}

#[allow(clippy::future_not_send)]
pub async fn with_serialized_messages(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: NonZeroU64,
    unsigned_tx: Vec<ProtobufAny>,
) -> Result<Response, Error> {
    let tx_bytes = signer
        .sign(
            TxBody::new(unsigned_tx, String::new(), 0_u32),
            calculate_fee(node_config, gas_limit),
        )
        .map(|signed_tx: RawTx| signed_tx.encode_to_vec())?;

    with_signed_body(client, tx_bytes, signer).await
}

#[allow(clippy::future_not_send)]
pub async fn with_gas_estimation(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    hard_gas_limit: NonZeroU64,
    fallback_gas_limit: NonZeroU64,
    unsigned_tx: ContractTx,
) -> Result<Response, ErrorWithEstimation> {
    let gas_limit = adjust_gas_limit(
        node_config,
        process_simulation_result(
            simulate(
                signer,
                client,
                node_config,
                hard_gas_limit,
                unsigned_tx.clone(),
            )
            .await,
            fallback_gas_limit,
        ),
        hard_gas_limit,
    );

    commit(signer, client, node_config, gas_limit, unsigned_tx)
        .await
        .map_err(From::from)
}

#[allow(clippy::future_not_send)]
pub async fn with_gas_estimation_and_serialized_message(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    hard_gas_limit: NonZeroU64,
    fallback_gas_limit: NonZeroU64,
    unsigned_tx: Vec<ProtobufAny>,
) -> Result<Response, ErrorWithEstimation> {
    let gas_limit = adjust_gas_limit(
        node_config,
        process_simulation_result(
            simulate::with_serialized_messages(
                signer,
                client,
                node_config,
                hard_gas_limit,
                unsigned_tx.clone(),
            )
            .await,
            fallback_gas_limit,
        ),
        hard_gas_limit,
    );

    with_serialized_messages(signer, client, node_config, gas_limit, unsigned_tx)
        .await
        .map_err(Into::into)
}

#[allow(clippy::future_not_send)]
pub async fn with_signed_body(
    client: &Client,
    tx_bytes: Vec<u8>,
    signer: &mut Signer,
) -> Result<Response, Error> {
    const SIGNATURE_VERIFICATION_ERROR: u32 = 4;

    let result = client
        .tx_service_client()
        .broadcast_tx(BroadcastTxRequest {
            tx_bytes,
            mode: BroadcastMode::Sync.into(),
        })
        .await
        .map(TonicResponse::into_inner);

    match result {
        Ok(BroadcastTxResponse {
            tx_response:
                Some(TxResponse {
                    txhash: tx_hash,
                    code,
                    raw_log,
                    info,
                    ..
                }),
        }) => {
            let code: TxCode = code.into();

            if !code.is_err() || code.value() != SIGNATURE_VERIFICATION_ERROR {
                signer.tx_confirmed();
            }

            Ok(Response {
                code,
                raw_log: raw_log.into_boxed_str(),
                info: info.into_boxed_str(),
                tx_hash: TxHash(tx_hash),
            })
        }
        Ok(BroadcastTxResponse { tx_response: None }) => Err(Error::EmptyResponseReceived),
        Err(status) => Err(Error::Broadcast(status)),
    }
}
