use std::num::NonZeroU32;

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
    },
    tendermint::abci::Code,
    tx::Fee,
};
use serde::de::DeserializeOwned;
use tracing::debug;

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
                .with_grpc(move |rpc| async move {
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
    client: &Client,
    address: &str,
    query: &[u8],
) -> Result<R, error::WasmQuery>
where
    R: DeserializeOwned,
{
    serde_json_wasm::from_slice::<R>(&{
        let data = client
            .with_grpc({
                let query_data = query.to_vec();

                move |rpc| async move {
                    WasmQueryClient::new(rpc)
                        .smart_contract_state(QuerySmartContractStateRequest {
                            address: address.into(),
                            query_data,
                        })
                        .await
                }
            })
            .await?
            .into_inner()
            .data;

        debug!(
            data = %String::from_utf8_lossy(&data),
            "gRPC query response from {address} returned successfully!",
        );

        data
    })
    .map_err(Into::into)
}

pub async fn simulate_tx(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<GasInfo, error::SimulateTx> {
    let simulation_tx = unsigned_tx
        .commit(
            signer,
            Fee::from_amount_and_gas(config.fee().clone(), gas_limit),
            None,
            None,
        )?
        .to_bytes()?;

    let gas_info: GasInfo = client
        .with_grpc(move |channel| async move {
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
    const ERROR_CODE: Code = Code::Err(if let Some(n) = NonZeroU32::new(13) {
        n
    } else {
        panic!()
    });

    let signed_tx = unsigned_tx.commit(
        signer,
        Fee::from_amount_and_gas(node_config.fee().clone(), gas_limit),
        None,
        None,
    )?;

    let tx_commit_response = client
        .with_json_rpc(|rpc| async move { signed_tx.broadcast_commit(&rpc).await })
        .await?;

    if !(tx_commit_response.deliver_tx.code == ERROR_CODE
        && tx_commit_response.deliver_tx.gas_used == 0
        && tx_commit_response.deliver_tx.gas_wanted == 0)
    {
        signer.tx_confirmed();
    }

    Ok(tx_commit_response)
}

pub async fn commit_tx_with_gas_estimation(
    signer: &mut Signer,
    client: &Client,
    node_config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<CommitResponse, error::GasEstimatingTxCommit> {
    let gas_info = simulate_tx(signer, client, node_config, gas_limit, unsigned_tx.clone()).await?;

    commit_tx(
        signer,
        client,
        node_config,
        unsigned_tx,
        u128::from(gas_info.gas_used)
            .checked_mul(node_config.gas_adjustment_numerator().into())
            .and_then(|result| result.checked_div(node_config.gas_adjustment_denominator().into()))
            .map(|result| u64::try_from(result).unwrap_or(u64::MAX))
            .unwrap_or(gas_info.gas_used),
    )
    .await
    .map_err(Into::into)
}
