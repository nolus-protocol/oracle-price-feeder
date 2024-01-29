use cosmrs::{
    rpc::{Client as _, HttpClient as RpcHttpClient},
    tx::{Body as TxBody, Raw as RawTx},
    Any,
};

use error::{CommitTx as Error, GasEstimatingTxCommit as ErrorWithEstimation};

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

use super::{
    adjust_gas_limit, calculate_fee, process_simulation_result, simulate, simulate::simulate,
};

pub mod error;

pub type Response = cosmrs::rpc::endpoint::broadcast::tx_sync::Response;

pub async fn commit(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<Response, Error> {
    let signed_tx = unsigned_tx
        .commit(signer, calculate_fee(node_config, gas_limit), None, None)
        .map_err(From::from)
        .and_then(|signed_tx: RawTx| signed_tx.to_bytes().map_err(Error::Serialize))?;

    with_signed_body(client, signed_tx, signer).await
}

pub async fn with_serialized_messages(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: u64,
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

pub async fn with_gas_estimation(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    hard_gas_limit: u64,
    fallback_gas_limit: u64,
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

pub async fn with_gas_estimation_and_serialized_message(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    hard_gas_limit: u64,
    fallback_gas_limit: u64,
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

    self::with_serialized_messages(signer, client, node_config, gas_limit, unsigned_tx)
        .await
        .map_err(Into::into)
}

pub async fn with_signed_body(
    client: &Client,
    signed_tx: Vec<u8>,
    signer: &mut Signer,
) -> Result<Response, Error> {
    const SIGNATURE_VERIFICATION_ERROR: u32 = 4;

    match client
        .with_json_rpc(|rpc: RpcHttpClient| async move { rpc.broadcast_tx_sync(signed_tx).await })
        .await
    {
        Ok(response) => {
            if !response.code.is_err() || response.code.value() != SIGNATURE_VERIFICATION_ERROR {
                signer.tx_confirmed();
            }

            Ok(response)
        }
        Err(error) => Err(Error::Broadcast(error)),
    }
}
