use anyhow::{Context as _, Result};
use cosmrs::proto::cosmos::bank::v1beta1::QueryBalanceRequest;

use super::{set_reconnect_if_required, QueryBank};

impl QueryBank {
    pub async fn balance(
        &mut self,
        address: String,
        denom: String,
    ) -> Result<u128> {
        const QUERY_BALANCE_ERROR: &str =
            "Failed to query balance information!";

        const MISSING_BALANCE_ERROR: &str =
            "Query response doesn't contain balance information!";

        const PARSE_BALANCE_ERROR: &str = "Failed to parse balance amount!";

        self.inner
            .bank_query_client()
            .await?
            .balance(QueryBalanceRequest { address, denom })
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_BALANCE_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .balance
                    .context(MISSING_BALANCE_ERROR)
                    .and_then(|balance| {
                        balance.amount.parse().context(PARSE_BALANCE_ERROR)
                    })
            })
    }
}
