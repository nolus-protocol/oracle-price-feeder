use anyhow::{anyhow, Context as _, Result};
use cosmrs::{
    auth::BaseAccount,
    proto::cosmos::auth::v1beta1::{
        BaseAccount as BaseAccountProtobuf, QueryAccountRequest,
    },
};

use super::{set_reconnect_if_required, QueryAuth};

impl QueryAuth {
    pub async fn account(&mut self, address: String) -> Result<BaseAccount> {
        const QUERY_ACCOUNT_DATA_ERROR: &str =
            "Failed to query account information!";

        const MISSING_ACCOUNT_DATA_ERROR: &str =
            "Query response doesn't contain account data!";

        const DECODE_ACCOUNT_DATA_ERROR: &str =
            "Failed to decode account data query's response from binary \
            Protobuf format!";

        const CONVERT_FROM_PROTOBUF_ERROR: &str =
            "Failed to convert account data query's response into it's \
            structured form!";

        self.inner
            .auth_query_client()
            .await?
            .account(QueryAccountRequest { address })
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_ACCOUNT_DATA_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .account
                    .context(MISSING_ACCOUNT_DATA_ERROR)
                    .and_then(|response| {
                        response
                            .to_msg::<BaseAccountProtobuf>()
                            .context(DECODE_ACCOUNT_DATA_ERROR)
                    })
                    .and_then(|base_account| {
                        BaseAccount::try_from(base_account)
                            .map_err(|error| anyhow!(error))
                            .context(CONVERT_FROM_PROTOBUF_ERROR)
                    })
            })
    }
}
