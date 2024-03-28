use tracing::{info, info_span};

use chain_comms::{
    interact::commit::Response, reexport::cosmrs::tendermint::abci::Code,
};
use logging::{debug_logs, error_logs};

pub fn commit_response(response: &Response) {
    info_span!("Mempool Response").in_scope(|| {
        info!("Hash: {}", response.tx_hash);

        on_error(response.code, &response.raw_log, &response.info);
    });
}

pub fn on_error(code: Code, raw_log: &str, info: &str) {
    if code.is_ok() {
        debug_logs(&[
            format_args!("Raw Log: {raw_log}"),
            format_args!("Info: {info}"),
        ]);
    } else {
        error_logs(&[
            format_args!("Raw Log: {raw_log}"),
            format_args!("Info: {info}"),
            format_args!("Error with code {} has occurred!", code.value()),
        ]);
    }
}
