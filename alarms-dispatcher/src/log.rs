use tracing::{error, info, info_span};

use chain_comms::{
    decode,
    interact::{get_tx_response::Response as TxResponse, TxHash},
};

use crate::messages::DispatchResponse;

pub fn tx_response(
    contract_type: &str,
    contract_address: &str,
    hash: &TxHash,
    tx_result: &TxResponse,
) -> Option<DispatchResponse> {
    info_span!("Tx Response")
        .in_scope(|| tx_response_inner(contract_type, contract_address, hash, tx_result))
}

fn tx_response_inner(
    contract_type: &str,
    contract_address: &str,
    hash: &TxHash,
    tx_result: &TxResponse,
) -> Option<DispatchResponse> {
    info!("Contract type: {contract_type}\nContract: {contract_address}\nHash: {hash}");

    let mut maybe_dispatch_response = None;

    broadcast::log::on_error(tx_result.code, &tx_result.raw_log, &tx_result.info);

    if tx_result.code.is_ok() {
        match decode::tx_response_data(tx_result) {
            Ok(dispatch_response) => {
                maybe_dispatch_response = deserialize_and_log(tx_result, &dispatch_response);
            }
            Err(error) => error!(
                error = ?error,
                "Failed to decode transaction response from the Protobuf \
                format! Cause: {error}",
            ),
        }
    }

    info!(
        "Gas limit for transacion: {gas_wanted}\nGas used: {gas_used}",
        gas_wanted = tx_result.gas_wanted,
        gas_used = tx_result.gas_used,
    );

    maybe_dispatch_response
}

fn deserialize_and_log(
    tx_result: &TxResponse,
    dispatch_response: &[u8],
) -> Option<DispatchResponse> {
    serde_json_wasm::from_slice::<DispatchResponse>(dispatch_response)
        .inspect(|dispatch_response| {
            info!(
                "Dispatched {} alarms.",
                dispatch_response.dispatched_alarms()
            );
        })
        .inspect_err(|error| {
            error!(
                error = ?error,
                response_data = tx_result.data,
                "Failed to deserialize transaction response from the JSON \
                format! Cause: {error}",
            );
        })
        .ok()
}
