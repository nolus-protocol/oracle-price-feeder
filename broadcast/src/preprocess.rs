use std::num::NonZeroU64;

use tracing::error;

use chain_comms::{
    config::Node as NodeConfig,
    interact::{
        adjust_gas_limit, calculate_fee, process_simulation_result, simulate,
    },
    reexport::cosmrs::{
        proto::prost::Message, tx::Body as TxBody, Any as ProtobufAny,
    },
    signer::Signer,
};

use crate::{cache, mode, ApiAndConfiguration};

#[inline]
#[allow(clippy::future_not_send)]
pub async fn next_tx_request<Impl>(
    api_and_configuration: &mut ApiAndConfiguration,
    requests_cache: &cache::TxRequests<Impl>,
    next_sender_id: &mut usize,
) -> Option<TxRequest<Impl>>
where
    Impl: mode::Impl,
{
    loop {
        let cache::GetNextResult {
            sender_id,
            tx_request:
                cache::TxRequest {
                    messages,
                    hard_gas_limit,
                    fallback_gas_limit,
                    expiration,
                },
            ..
        }: cache::GetNextResult<Impl> =
            cache::get_next(requests_cache, *next_sender_id)?;

        *next_sender_id = sender_id.wrapping_add(1);

        let Some(Output { signed_tx_bytes }) = preprocess::<Impl>(
            api_and_configuration,
            fallback_gas_limit,
            messages,
            hard_gas_limit,
        )
        .await
        else {
            continue;
        };

        break Some(TxRequest {
            sender_id,
            signed_tx_bytes,
            expiration,
        });
    }
}

pub(crate) struct TxRequest<Impl: mode::Impl> {
    pub(crate) sender_id: usize,
    pub(crate) signed_tx_bytes: Vec<u8>,
    pub(crate) expiration: Impl::Expiration,
}

#[inline]
#[allow(clippy::future_not_send)]
async fn preprocess<Impl: mode::Impl>(
    ApiAndConfiguration {
        node_client,
        node_config,
        signer,
        ..
    }: &mut ApiAndConfiguration,
    fallback_gas_limit: NonZeroU64,
    messages: Vec<ProtobufAny>,
    hard_gas_limit: NonZeroU64,
) -> Option<Output> {
    let tx_body: TxBody = TxBody::new(messages, String::new(), 0_u32);

    let signed_tx_bytes: Vec<u8> = sign_and_serialize_tx(
        signer,
        node_config,
        hard_gas_limit,
        tx_body.clone(),
    )?;

    let simulation_result = simulate::with_signed_body(
        node_client,
        signed_tx_bytes,
        hard_gas_limit,
    )
    .await;

    let estimated_gas_limit: NonZeroU64 =
        process_simulation_result(simulation_result, fallback_gas_limit);

    let gas_limit: NonZeroU64 =
        adjust_gas_limit(node_config, estimated_gas_limit, hard_gas_limit);

    sign_and_serialize_tx(signer, node_config, gas_limit, tx_body)
        .map(|signed_tx_bytes| Output { signed_tx_bytes })
}

struct Output {
    pub(crate) signed_tx_bytes: Vec<u8>,
}

fn sign_and_serialize_tx(
    signer: &Signer,
    node_config: &NodeConfig,
    gas_limit: NonZeroU64,
    tx_body: TxBody,
) -> Option<Vec<u8>> {
    signer
        .sign(tx_body, calculate_fee(node_config, gas_limit))
        .inspect_err(|error| {
            error!(error = ?error, "Signing transaction failed! Cause: {}", error);
        })
        .ok()
        .as_ref()
        .map(Message::encode_to_vec)
}
