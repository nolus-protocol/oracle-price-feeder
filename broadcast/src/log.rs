use tracing::{debug, error, info, info_span};

use chain_comms::{interact::commit::Response, reexport::cosmrs::tendermint::abci::Code};

pub fn commit_response(response: &Response) {
    info_span!("Mempool Response").in_scope(|| {
        info!("Hash: {}", response.tx_hash);

        on_error(response.code, &response.raw_log, &response.info);
    });
}

pub fn on_error(code: Code, raw_log: &str, info: &str) {
    if code.is_ok() {
        debug!("Raw Log: {}", raw_log);

        debug!("Info: {}", info);
    } else {
        error!("Raw Log: {}", raw_log);

        error!("Info: {}", info);

        error!("Error with code {} has occurred!", code.value());
    }
}
