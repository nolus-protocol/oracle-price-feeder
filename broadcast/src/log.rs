use tracing::{debug, error, info, info_span};

use chain_comms::{interact::commit::Response, reexport::cosmrs::tendermint::abci::Code};

pub fn commit_response(response: &Response) {
    info_span!("Mempool Response").in_scope(|| {
        info!("Hash: {}", response.hash);

        on_error(response.code, &response.log);
    });
}

pub fn on_error(code: Code, log: &str) {
    if code.is_ok() {
        debug!("Log: {}", log);
    } else {
        error!(log = log, "Error with code {} has occurred!", code.value());
    }
}
