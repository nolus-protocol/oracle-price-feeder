use std::num::NonZeroU64;

use cosmrs::{
    rpc::{error::Error as RpcError, Client as _, HttpClient as RpcHttpClient},
    tx::{Body as TxBody, Raw as RawTx},
    Any,
};

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

use super::{
    adjust_gas_limit, calculate_fee, process_simulation_result, simulate, simulate::simulate,
};

use self::error::{CommitTx as Error, GasEstimatingTxCommit as ErrorWithEstimation};

pub mod error;

pub type Response = cosmrs::rpc::endpoint::broadcast::tx_sync::Response;

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
        .map_err(From::from)
        .and_then(|signed_tx: RawTx| signed_tx.to_bytes().map_err(Error::Serialize))?;

    with_signed_body(client, signed_tx, signer).await
}

#[allow(clippy::future_not_send)]
pub async fn with_serialized_messages(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: NonZeroU64,
    unsigned_tx: Vec<Any>,
) -> Result<Response, Error> {
    let signed_tx = signer
        .sign(
            TxBody::new(unsigned_tx, String::new(), 0_u32),
            calculate_fee(node_config, gas_limit),
        )
        .map_err(From::from)
        .and_then(|signed_tx: RawTx| signed_tx.to_bytes().map_err(Error::Serialize))?;

    with_signed_body(client, signed_tx, signer).await
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
    unsigned_tx: Vec<Any>,
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
    signed_tx: Vec<u8>,
    signer: &mut Signer,
) -> Result<Response, Error> {
    const SIGNATURE_VERIFICATION_ERROR: u32 = 4;

    let response_result: Result<Response, RpcError> = client
        .with_json_rpc(|rpc: RpcHttpClient| async move { rpc.broadcast_tx_sync(signed_tx).await })
        .await;

    match response_result {
        Ok(response) => {
            if !response.code.is_err() || response.code.value() != SIGNATURE_VERIFICATION_ERROR {
                signer.tx_confirmed();
            }

            Ok(response)
        }
        Err(error) => Err(Error::Broadcast(error)),
    }
}
