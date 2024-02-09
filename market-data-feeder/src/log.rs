use tracing::{info, info_span};

use chain_comms::interact::{get_tx_response::Response as TxResponse, TxHash};

pub fn tx_response(provider_id: &str, hash: &TxHash, tx_result: &TxResponse) {
    info_span!("Tx Response", provider_name = provider_id).in_scope(|| {
        info!("Hash: {}", hash);

        broadcast::log::on_error(tx_result.code, &tx_result.raw_log, &tx_result.info);

        info!("Gas limit for transaction: {}", tx_result.gas_wanted);
        info!("Gas used: {}", tx_result.gas_used);
    });
}
