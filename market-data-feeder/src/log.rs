use tracing::{info, info_span};

use chain_comms::reexport::cosmrs::tendermint::{abci::response::DeliverTx, Hash};

pub fn tx_response(provider_name: &str, hash: &Hash, response: &DeliverTx) {
    info_span!("Tx Response", provider_name = provider_name).in_scope(|| {
        info!("Hash: {}", hash);

        broadcast::log::on_error(response.code, &response.log);

        info!("Gas limit for transacion: {}", response.gas_wanted);
        info!("Gas used: {}", response.gas_used);
    });
}
