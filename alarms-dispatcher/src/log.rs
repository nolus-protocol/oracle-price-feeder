use tracing::{debug, error, info, info_span};

use chain_comms::{
    decode,
    interact::commit::Response,
    reexport::cosmrs::tendermint::{abci::response::DeliverTx, Hash},
};

use crate::messages::DispatchResponse;

pub fn commit_response(response: &Response) {
    info_span!("Mempool Response").in_scope(|| {
        info!("Hash: {}", response.hash);

        if response.code.is_ok() {
            debug!("Log: {}", response.log);
        } else {
            error!(
                log = response.log,
                "Error with code {} has occurred!",
                response.code.value(),
            );
        }
    });
}

pub fn tx_response(
    contract_type: &str,
    contract_address: &str,
    hash: &Hash,
    response: &DeliverTx,
) -> Option<DispatchResponse> {
    info_span!("Tx Response")
        .in_scope(|| {
            info!("Contract type: {}", contract_type);

            info!("Contract: {}", contract_address);

            info!("Hash: {hash}");

            let mut maybe_dispatch_response = None;

            if response.code.is_ok() {
                debug!("Log: {}", response.log);

                match decode::exec_tx_data(response) {
                    Ok(dispatch_response) => {
                        match serde_json_wasm::from_slice::<DispatchResponse>(&dispatch_response) {
                            Ok(dispatch_response) => {
                                info!(
                                    "Dispatched {} alarms.",
                                    dispatch_response.dispatched_alarms()
                                );

                                maybe_dispatch_response = Some(dispatch_response);
                            }
                            Err(error) => error!(
                                error = ?error,
                                response_data = String::from_utf8_lossy(&response.data).as_ref(),
                                "Failed to deserialize transaction response from the JSON format! Cause: {error}",
                            ),
                        }
                    }
                    Err(error) => error!(
                        error = ?error,
                        "Failed to decode transaction response from the Protobuf format! Cause: {error}",
                    ),
                }
            } else {
                error!(
                    log = response.log,
                    "Error with code {} has occurred!",
                    response.code.value(),
                );
            }

            info!("Gas limit for transacion: {}", response.gas_wanted);
            info!("Gas used: {}", response.gas_used);

            maybe_dispatch_response
        })
}
