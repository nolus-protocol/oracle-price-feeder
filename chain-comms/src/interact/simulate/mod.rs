use std::{future::Future, num::NonZeroU64};

use cosmrs::{
    proto::{
        cosmos::{
            base::abci::v1beta1::GasInfo,
            tx::v1beta1::{SimulateRequest, SimulateResponse},
        },
        prost::Message as _,
        Any as ProtobufAny,
    },
    tx::Body as TxBody,
};
use tonic::Response as TonicResponse;

use crate::{build_tx::ContractTx, client::Client, config::Node, signer::Signer};

use super::calculate_fee;

use self::error::Error;

pub mod error;

pub fn simulate<'r>(
    signer: &mut Signer,
    client: &'r Client,
    config: &'r Node,
    gas_limit: NonZeroU64,
    unsigned_tx: ContractTx,
) -> impl Future<Output = Result<GasInfo, Error>> + Send + 'r {
    let simulation_tx_result: Result<Vec<u8>, Error> = unsigned_tx
        .commit(signer, calculate_fee(config, gas_limit), None, None)
        .map_err(Error::Commit)
        .map(|tx| tx.encode_to_vec());

    async move { with_signed_body(client, simulation_tx_result?, gas_limit).await }
}

pub fn with_serialized_messages<'r>(
    signer: &mut Signer,
    client: &'r Client,
    config: &'r Node,
    gas_limit: NonZeroU64,
    unsigned_tx: Vec<ProtobufAny>,
) -> impl Future<Output = Result<GasInfo, Error>> + Send + 'r {
    let simulation_tx_result: Result<Vec<u8>, Error> = signer
        .sign(
            TxBody::new(unsigned_tx, String::new(), 0_u32),
            calculate_fee(config, gas_limit),
        )
        .map_err(Error::Signing)
        .map(|tx| tx.encode_to_vec());

    async move { with_signed_body(client, simulation_tx_result?, gas_limit).await }
}

pub async fn with_signed_body(
    client: &Client,
    simulation_tx: Vec<u8>,
    hard_gas_limit: NonZeroU64,
) -> Result<GasInfo, Error> {
    client
        .tx_service_client()
        .simulate(SimulateRequest {
            tx_bytes: simulation_tx,
            ..Default::default()
        })
        .await
        .map(TonicResponse::into_inner)
        .map_err(Error::SimulationRunError)
        .and_then(|SimulateResponse { gas_info, .. }| {
            gas_info.ok_or(Error::MissingSimulationGasInto)
        })
        .and_then(|gas_info| {
            if gas_info.gas_used <= hard_gas_limit.get() {
                Ok(gas_info)
            } else {
                Err(Error::SimulationGasExceedsLimit {
                    used: gas_info.gas_used,
                })
            }
        })
}
