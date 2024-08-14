use anyhow::{Context as _, Result};
use cosmrs::proto::cosmos::base::reflection::v2alpha1::GetConfigurationDescriptorRequest;

use crate::node::{set_reconnect_if_required, QueryReflection};

impl QueryReflection {
    pub async fn account_prefix(&mut self) -> Result<String> {
        const QUERY_CONFIGURATION_DESCRIPTOR_ERROR: &str =
            "Failed to query network's configuration descriptor!";

        const MISSING_ACCOUNT_PREFIX_ERROR: &str =
            "Query response doesn't contain account address prefix \
            configuration!";

        self.inner
            .reflection_service_client()
            .await?
            .get_configuration_descriptor(GetConfigurationDescriptorRequest {})
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_CONFIGURATION_DESCRIPTOR_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .config
                    .map(|configuration| {
                        configuration.bech32_account_address_prefix
                    })
                    .context(MISSING_ACCOUNT_PREFIX_ERROR)
            })
    }
}
