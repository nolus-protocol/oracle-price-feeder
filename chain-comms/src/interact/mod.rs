use cosmrs::{
    proto::{
        cosmos::{
            auth::v1beta1::{
                query_client::QueryClient as AuthQueryClient, BaseAccount, QueryAccountRequest,
            },
            base::abci::v1beta1::GasInfo,
            tx::v1beta1::{service_client::ServiceClient, SimulateRequest},
        },
        cosmwasm::wasm::v1::{
            query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
        },
        prost,
    },
    rpc::{Client as _, HttpClient as RpcHttpClient},
    tendermint::{abci::response::DeliverTx, Hash},
    tx::{Body as TxBody, Fee, Raw as RawTx},
    Any, Coin,
};
use serde::de::DeserializeOwned;
use tonic::{client::Grpc as GrpcClient, transport::Channel as TonicChannel, IntoRequest as _};
use tracing::{debug, error};

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

pub mod error;

pub type CommitResponse = cosmrs::rpc::endpoint::broadcast::tx_sync::Response;

pub async fn query_account_data(
    client: &Client,
    address: &str,
) -> Result<BaseAccount, error::AccountQuery> {
    prost::Message::decode(
        {
            let data = client
                .with_grpc(move |rpc: TonicChannel| async move {
                    AuthQueryClient::new(rpc)
                        .account(QueryAccountRequest {
                            address: address.into(),
                        })
                        .await
                })
                .await?
                .into_inner()
                .account
                .ok_or(error::AccountQuery::NoAccountData)?
                .value;

            debug!("gRPC query response from {address} returned successfully!");

            data
        }
        .as_slice(),
    )
    .map_err(Into::into)
}

pub async fn raw_query<Q, R>(
    rpc: TonicChannel,
    query: Q,
    type_url: &'static str,
) -> Result<R, error::RawQuery>
where
    Q: prost::Message + 'static,
    R: prost::Message + Default + 'static,
{
    let mut grpc_client: GrpcClient<TonicChannel> = GrpcClient::new(rpc.clone());

    grpc_client.ready().await?;

    grpc_client
        .unary(
            query.into_request(),
            http::uri::PathAndQuery::from_static(type_url),
            tonic::codec::ProstCodec::default(),
        )
        .await
        .map(tonic::Response::into_inner)
        .map_err(error::RawQuery::Response)
}

pub async fn query_wasm<R>(
    rpc: TonicChannel,
    address: String,
    query: &[u8],
) -> Result<R, error::WasmQuery>
where
    R: DeserializeOwned,
{
    WasmQueryClient::new(rpc)
        .smart_contract_state(QuerySmartContractStateRequest {
            address,
            query_data: query.to_vec(),
        })
        .await
        .map_err(|error| error::WasmQuery::RawQuery(error::RawQuery::Response(error)))
        .and_then(|response| {
            serde_json_wasm::from_slice(&response.into_inner().data).map_err(From::from)
        })
}

pub async fn simulate_tx(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<GasInfo, error::SimulateTx> {
    simulate_tx_with_signed_body(
        client,
        unsigned_tx
            .commit(signer, calculate_fee(config, gas_limit)?, None, None)?
            .to_bytes()?,
        gas_limit,
    )
    .await
}

pub async fn simulate_tx_with_serialized_messages(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    gas_limit: u64,
    unsigned_tx: Vec<Any>,
) -> Result<GasInfo, error::SimulateTx> {
    simulate_tx_with_signed_body(
        client,
        signer
            .sign(
                TxBody::new(unsigned_tx, String::new(), 0_u32),
                calculate_fee(config, gas_limit)?,
            )?
            .to_bytes()?,
        gas_limit,
    )
    .await
}

pub fn adjust_gas_limit(node_config: &Node, gas_limit: u64, hard_gas_limit: u64) -> u64 {
    u128::from(gas_limit)
        .checked_mul(node_config.gas_adjustment_numerator().get().into())
        .and_then(|result: u128| {
            result.checked_div(node_config.gas_adjustment_denominator().get().into())
        })
        .map_or(gas_limit, |result: u128| {
            u64::try_from(result).unwrap_or(u64::MAX)
        })
        .min(hard_gas_limit)
}

pub async fn commit_tx(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<CommitResponse, error::CommitTx> {
    let signed_tx = unsigned_tx
        .commit(signer, calculate_fee(node_config, gas_limit)?, None, None)
        .map_err(From::from)
        .and_then(|signed_tx: RawTx| signed_tx.to_bytes().map_err(error::CommitTx::Serialize))?;

    commit_tx_with_signed_body(client, signed_tx, signer).await
}

pub async fn commit_tx_with_serialized_message(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: u64,
    unsigned_tx: Vec<Any>,
) -> Result<CommitResponse, error::CommitTx> {
    let signed_tx = signer
        .sign(
            TxBody::new(unsigned_tx, String::new(), 0_u32),
            calculate_fee(node_config, gas_limit)?,
        )
        .map_err(From::from)
        .and_then(|signed_tx: RawTx| signed_tx.to_bytes().map_err(error::CommitTx::Serialize))?;

    commit_tx_with_signed_body(client, signed_tx, signer).await
}

pub async fn commit_tx_with_gas_estimation(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    hard_gas_limit: u64,
    fallback_gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<CommitResponse, error::GasEstimatingTxCommit> {
    let gas_limit = adjust_gas_limit(
        node_config,
        process_simulation_result(
            simulate_tx(
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

    commit_tx(signer, client, node_config, gas_limit, unsigned_tx)
        .await
        .map_err(Into::into)
}

pub async fn commit_tx_with_gas_estimation_and_serialized_message(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    hard_gas_limit: u64,
    fallback_gas_limit: u64,
    unsigned_tx: Vec<Any>,
) -> Result<CommitResponse, error::GasEstimatingTxCommit> {
    let gas_limit = adjust_gas_limit(
        node_config,
        process_simulation_result(
            simulate_tx_with_serialized_messages(
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

    commit_tx_with_serialized_message(signer, client, node_config, gas_limit, unsigned_tx)
        .await
        .map_err(Into::into)
}

pub async fn get_tx_response(
    client: &Client,
    tx_hash: Hash,
) -> Result<DeliverTx, error::GetTxResponse> {
    client
        .with_json_rpc(move |rpc| async move { rpc.tx(tx_hash, false).await })
        .await
        .map(|response| response.tx_result)
        .map_err(From::from)
}

async fn simulate_tx_with_signed_body(
    client: &Client,
    simulation_tx: Vec<u8>,
    gas_limit: u64,
) -> Result<GasInfo, error::SimulateTx> {
    let gas_info: GasInfo = client
        .with_grpc(move |channel: TonicChannel| async move {
            ServiceClient::new(channel)
                .simulate(SimulateRequest {
                    tx_bytes: simulation_tx,
                    ..Default::default()
                })
                .await
        })
        .await?
        .into_inner()
        .gas_info
        .ok_or(error::SimulateTx::MissingSimulationGasInto)?;

    if gas_limit < gas_info.gas_used {
        return Err(error::SimulateTx::SimulationGasExceedsLimit {
            used: gas_info.gas_used,
        });
    }

    Ok(gas_info)
}

fn process_simulation_result(
    simulated_tx_result: Result<GasInfo, error::SimulateTx>,
    fallback_gas_limit: u64,
) -> u64 {
    match simulated_tx_result {
        Ok(gas_info) => gas_info.gas_used,
        Err(error) => {
            error!(
                error = %error,
                "Failed to simulate transaction! Falling back to provided gas limit. Fallback gas limit: {gas_limit}.",
                gas_limit = fallback_gas_limit
            );

            fallback_gas_limit
        }
    }
}

async fn commit_tx_with_signed_body(
    client: &Client,
    signed_tx: Vec<u8>,
    signer: &mut Signer,
) -> Result<CommitResponse, error::CommitTx> {
    const COSMWASM_WASM_CODESPACE_ERROR_CODE: u32 = 5;

    client
        .with_json_rpc(|rpc: RpcHttpClient| async move { rpc.broadcast_tx_sync(signed_tx).await })
        .await
        .map_err(From::from)
        .map(|response| {
            if response.code.is_ok()
                || (response.code.is_err()
                    && matches!(response.code.value(), COSMWASM_WASM_CODESPACE_ERROR_CODE))
            {
                signer.tx_confirmed();
            }

            response
        })
        .map_err(error::CommitTx::Broadcast)
}

fn calculate_fee(config: &Node, gas_limit: u64) -> Result<Fee, error::FeeCalculation> {
    Ok(Fee::from_amount_and_gas(
        Coin::new(
            u128::from(gas_limit)
                .saturating_mul(config.gas_price_numerator().get().into())
                .saturating_div(config.gas_price_denominator().get().into())
                .saturating_mul(config.fee_adjustment_numerator().get().into())
                .saturating_div(config.fee_adjustment_denominator().get().into()),
            config.fee_denom(),
        )?,
        gas_limit,
    ))
}
