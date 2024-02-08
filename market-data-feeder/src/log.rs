use tracing::{info, info_span};

use chain_comms::reexport::cosmrs::tendermint::{abci::types::ExecTxResult, Hash};

pub fn tx_response(provider_id: &str, hash: &Hash, tx_result: &ExecTxResult) {
    info_span!("Tx Response", provider_name = provider_id).in_scope(|| {
        info!("Hash: {}", hash);

        broadcast::log::on_error(tx_result.code, &tx_result.log);

        info!("Gas limit for transacion: {}", tx_result.gas_wanted);
        info!("Gas used: {}", tx_result.gas_used);
    });
}
