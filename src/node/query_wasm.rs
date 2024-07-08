use std::any::type_name;

use anyhow::{Context as _, Result};
use cosmrs::proto::cosmwasm::wasm::v1::QuerySmartContractStateRequest;
use serde::de::DeserializeOwned;

use super::{set_reconnect_if_required, QueryWasm};

impl QueryWasm {
    pub async fn smart<T>(
        &mut self,
        address: String,
        query_data: Vec<u8>,
    ) -> Result<T>
    where
        T: DeserializeOwned,
    {
        const QUERY_CONTRACT_ERROR: &str =
            "Failed to run query against contract!";

        self.inner
            .wasm_query_client()
            .await?
            .smart_contract_state(QuerySmartContractStateRequest {
                address,
                query_data,
            })
            .await
            .map(|response| response.into_inner().data)
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_CONTRACT_ERROR)
            .and_then(|data| {
                serde_json_wasm::from_slice(&data)
                    .with_context(|| {
                        format!(
                            "Response data: {}",
                            String::from_utf8_lossy(&data),
                        )
                    })
                    .with_context(|| {
                        format!(
                            r#"Failed to deserialize response into "{}"!"#,
                            type_name::<T>()
                        )
                    })
            })
    }
}
