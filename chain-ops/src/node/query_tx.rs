use anyhow::{Context as _, Result};
use cosmrs::proto::cosmos::{
    base::abci::v1beta1::TxResponse, tx::v1beta1::GetTxRequest,
};

use super::{set_reconnect_if_required, QueryTx};

impl QueryTx {
    pub async fn tx(&mut self, hash: String) -> Result<Option<TxResponse>> {
        const MISSING_RESPONSE_ERROR: &str =
            "Query response doesn't contain transaction result!";

        let result = self
            .inner
            .tx_service_client()
            .await?
            .get_tx(GetTxRequest { hash })
            .await;

        match result {
            Ok(response) => response
                .into_inner()
                .tx_response
                .context(MISSING_RESPONSE_ERROR)
                .map(Some),
            Err(status)
                if matches!(status.code(), tonic::Code::NotFound {}) =>
            {
                Ok(None)
            },
            Err(status) => {
                set_reconnect_if_required(&self.inner, status.code());

                Err(status.into())
            },
        }
    }
}
