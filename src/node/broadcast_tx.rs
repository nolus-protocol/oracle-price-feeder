use anyhow::{anyhow, Context as _, Result};
use cosmrs::{
    proto::cosmos::{
        base::abci::v1beta1::TxResponse,
        tx::v1beta1::{BroadcastMode, BroadcastTxRequest, SimulateRequest},
    },
    tx::Raw as RawTx,
    Gas,
};

use super::{set_reconnect_if_required, BroadcastTx};

impl BroadcastTx {
    const ENCODE_TRANSACTION_ERROR: &'static str =
        "Failed to encode signed transaction in binary Protobuf format!";

    pub async fn simulate(&mut self, tx: RawTx) -> Result<Gas> {
        const SIMULATE_TRANSACTION_ERROR: &str =
            "Failed to simulate transaction!";

        const MISSING_GAS_INFO_ERROR: &str =
            "Node didn't respond with gas information about simulation!";

        self.inner
            .tx_service_client()
            .await?
            .simulate(SimulateRequest {
                tx_bytes: {
                    tx.to_bytes()
                        .map_err(|error| anyhow!(error))
                        .context(Self::ENCODE_TRANSACTION_ERROR)?
                },
                ..Default::default()
            })
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(SIMULATE_TRANSACTION_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .gas_info
                    .map(|gas_info| gas_info.gas_used)
                    .context(MISSING_GAS_INFO_ERROR)
            })
    }

    pub async fn sync(&mut self, tx: RawTx) -> Result<TxResponse> {
        const BROADCAST_TRANSACTION_ERROR: &str =
            "Failed to broadcast transaction!";

        const MISSING_TRANSACTION_RESPONSE_ERROR: &str =
            "Node didn't respond with transaction response!";

        self.inner
            .tx_service_client()
            .await?
            .broadcast_tx(BroadcastTxRequest {
                tx_bytes: {
                    tx.to_bytes()
                        .map_err(|error| anyhow!(error))
                        .context(Self::ENCODE_TRANSACTION_ERROR)?
                },
                mode: BroadcastMode::Sync.into(),
            })
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(BROADCAST_TRANSACTION_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .tx_response
                    .context(MISSING_TRANSACTION_RESPONSE_ERROR)
            })
    }
}
