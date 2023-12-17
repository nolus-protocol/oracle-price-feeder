use chain_comms::{
    decode,
    interact::CommitResponse,
    reexport::cosmrs::tendermint::{abci::response::DeliverTx, Hash},
};
use tracing::{debug, error, info, info_span};

use crate::messages::DispatchResponse;

pub fn commit_response(contract_type: &str, contract_address: &str, response: &CommitResponse) {
    info_span!(
        "Mempool Response",
        contract_type = contract_type,
        contract_address = contract_address,
    )
    .in_scope(|| {
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

pub fn tx_response(contract_type: &str, contract_address: &str, hash: &Hash, response: &DeliverTx) {
    info_span!(
        "Tx Response",
        contract_type = contract_type,
        contract_address = contract_address,
    )
    .in_scope(|| {
        info!("Hash: {hash}");

        if response.code.is_ok() {
            debug!("Log: {}", response.log);

            match decode::exec_tx_data(response) {
                Ok(dispatch_response) => {
                    match serde_json_wasm::from_slice::<DispatchResponse>(&dispatch_response) {
                        Ok(dispatch_response) => info!(
                            "Dispatched {} alarms.",
                            dispatch_response.dispatched_alarms()
                        ),
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
    });
}
