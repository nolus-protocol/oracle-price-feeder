use tracing::{debug, error, info, info_span};

use chain_comms::{
    interact::commit::Response,
    reexport::cosmrs::tendermint::{abci::response::DeliverTx, Hash},
};

pub fn commit_response(provider_id: &str, response: &Response) {
    info_span!("Mempool Response", provider_id = provider_id).in_scope(|| {
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

pub fn tx_response(provider_id: &str, hash: &Hash, response: &DeliverTx) {
    info_span!("Tx Response", provider_id = provider_id).in_scope(|| {
        info!("Hash: {}", hash);

        if response.code.is_ok() {
            debug!("Log: {}", response.log);
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
