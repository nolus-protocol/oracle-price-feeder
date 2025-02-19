use anyhow::{Context as _, Result};
use cosmrs::{
    proto::cosmos::base::tendermint::v1beta1::{
        GetLatestBlockRequest, GetNodeInfoRequest, GetSyncingRequest,
    },
    tendermint::chain::Id as ChainId,
};

use super::{set_reconnect_if_required, QueryTendermint};

impl QueryTendermint {
    pub async fn chain_id(&mut self) -> Result<ChainId> {
        const QUERY_NODE_INFO_ERROR: &str =
            "Failed to query information about connected node!";

        const MISSING_DEFAULT_NODE_INFO_ERROR: &str =
            "Query response doesn't contain the default node information!";

        const PARSE_CHAIN_ID_ERROR: &str = "Failed to parse chain's ID!";

        self.inner
            .tendermint_service_client()
            .await?
            .get_node_info(GetNodeInfoRequest {})
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_NODE_INFO_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .default_node_info
                    .context(MISSING_DEFAULT_NODE_INFO_ERROR)
            })
            .and_then(|node_info| {
                node_info.network.parse().context(PARSE_CHAIN_ID_ERROR)
            })
    }

    pub async fn syncing(&mut self) -> Result<bool> {
        const QUERY_SYNCING_STATUS_ERROR: &str =
            "Failed to query syncing status of node!";

        self.inner
            .tendermint_service_client()
            .await?
            .get_syncing(GetSyncingRequest {})
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_SYNCING_STATUS_ERROR)
            .map(|response| response.into_inner().syncing)
    }

    pub async fn get_latest_block(&mut self) -> Result<u64> {
        const QUERY_NODE_INFO_ERROR: &str =
            "Failed to query node's latest block!";

        const MISSING_BLOCK_INFO_ERROR: &str =
            "Query response doesn't contain block information!";

        const MISSING_BLOCK_HEADER_INFO_ERROR: &str =
            "Query response doesn't contain block's header information!";

        self.inner
            .tendermint_service_client()
            .await?
            .get_latest_block(GetLatestBlockRequest {})
            .await
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(QUERY_NODE_INFO_ERROR)
            .and_then(|response| {
                response
                    .into_inner()
                    .sdk_block
                    .context(MISSING_BLOCK_INFO_ERROR)
                    .and_then(|block| {
                        block
                            .header
                            .map(|header| header.height.unsigned_abs())
                            .context(MISSING_BLOCK_HEADER_INFO_ERROR)
                    })
            })
    }
}
