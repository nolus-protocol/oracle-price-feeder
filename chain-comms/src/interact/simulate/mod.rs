use cosmrs::{
    proto::cosmos::{
        base::abci::v1beta1::GasInfo,
        tx::v1beta1::{service_client::ServiceClient, SimulateRequest},
    },
    tx::Body as TxBody,
    Any,
};
use tonic::transport::Channel as TonicChannel;

use error::Error;

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

use super::calculate_fee;

pub mod error;

pub async fn simulate(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    gas_limit: u64,
    unsigned_tx: ContractTx,
) -> Result<GasInfo, Error> {
    with_signed_body(
        client,
        unsigned_tx
            .commit(signer, calculate_fee(config, gas_limit), None, None)?
            .to_bytes()?,
        gas_limit,
    )
    .await
}

pub async fn with_serialized_messages(
    signer: &mut Signer,
    client: &Client,
    config: &Node,
    gas_limit: u64,
    unsigned_tx: Vec<Any>,
) -> Result<GasInfo, Error> {
    with_signed_body(
        client,
        signer
            .sign(
                TxBody::new(unsigned_tx, String::new(), 0_u32),
                calculate_fee(config, gas_limit),
            )?
            .to_bytes()?,
        gas_limit,
    )
    .await
}

pub async fn with_signed_body(
    client: &Client,
    simulation_tx: Vec<u8>,
    hard_gas_limit: u64,
) -> Result<GasInfo, Error> {
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
        .ok_or(Error::MissingSimulationGasInto)?;

    if hard_gas_limit < gas_info.gas_used {
        return Err(Error::SimulationGasExceedsLimit {
            used: gas_info.gas_used,
        });
    }

    Ok(gas_info)
}
