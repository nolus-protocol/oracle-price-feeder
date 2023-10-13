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
    rpc::HttpClient as RpcHttpClient,
    tx::{Fee, Raw as RawTx},
    Coin,
};
use serde::de::DeserializeOwned;
use tonic::transport::Channel as TonicChannel;
use tracing::{debug, error};

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

pub mod error;

pub type CommitResponse = cosmrs::rpc::endpoint::broadcast::tx_commit::Response;

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

pub async fn query_wasm<R>(
    rpc: TonicChannel,
    address: String,
    query: &[u8],
) -> Result<R, error::WasmQuery>
where
    R: DeserializeOwned,
{
    serde_json_wasm::from_slice(
        &WasmQueryClient::new(rpc)
            .smart_contract_state(QuerySmartContractStateRequest {
                address,
                query_data: query.to_vec(),
            })
            .await?
            .into_inner()
            .data,
    )
    .map_err(Into::into)
}

pub async fn simulate_tx(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<GasInfo, error::SimulateTx> {
    let simulation_tx: Vec<u8> = unsigned_tx
        .commit(signer, calculate_fee(config, gas_limit)?, None, None)?
        .to_bytes()?;

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

pub async fn commit_tx(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    unsigned_tx: ContractTx,
    gas_limit: u64,
) -> Result<CommitResponse, error::CommitTx> {
    let signed_tx: RawTx =
        unsigned_tx.commit(signer, calculate_fee(node_config, gas_limit)?, None, None)?;

    match client
        .with_json_rpc(|rpc: RpcHttpClient| async move { signed_tx.broadcast_commit(&rpc).await })
        .await
    {
        Ok(response) => {
            (if response.check_tx.code.is_ok() && response.deliver_tx.code.is_ok() {
                Signer::tx_confirmed
            } else {
                Signer::set_needs_update
            })(signer);

            Ok(response)
        }
        Err(error) => {
            signer.set_needs_update();

            Err(error.into())
        }
    }
}

pub async fn commit_tx_with_gas_estimation(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
    fallback_gas_limit: Option<u64>,
) -> Result<CommitResponse, error::GasEstimatingTxCommit> {
    let tx_gas_limit: u64 = match simulate_tx(
        signer,
        client,
        node_config,
        gas_limit,
        unsigned_tx.clone(),
    )
    .await
    {
        Ok(gas_info) => gas_info.gas_used,
        Err(error) => {
            let fallback_gas_limit: u64 = fallback_gas_limit.unwrap_or(gas_limit);

            error!(
                error = %error,
                "Failed to simulate transaction! Falling back to provided gas limit. Fallback gas limit: {gas_limit}.",
                gas_limit = fallback_gas_limit
            );

            fallback_gas_limit
        }
    };

    let adjusted_gas_limit: u64 = u128::from(tx_gas_limit)
        .checked_mul(node_config.gas_adjustment_numerator().get().into())
        .and_then(|result: u128| {
            result.checked_div(node_config.gas_adjustment_denominator().get().into())
        })
        .map_or(tx_gas_limit, |result| {
            u64::try_from(result).unwrap_or(u64::MAX)
        });

    commit_tx(signer, client, node_config, unsigned_tx, adjusted_gas_limit)
        .await
        .map_err(Into::into)
}

fn calculate_fee(config: &Node, gas_limit: u64) -> Result<Fee, error::FeeCalculation> {
    Ok(Fee::from_amount_and_gas(
        Coin::new(
            u128::from(gas_limit)
                .saturating_mul(config.gas_price_numerator().get().into())
                .saturating_div(config.gas_price_denominator().get().into()),
            config.fee_denom(),
        )?,
        gas_limit,
    ))
}
