use anyhow::{Context as _, Result};
use prost::Message;
use tonic::{codec::ProstCodec, codegen::http::uri::PathAndQuery, IntoRequest};

use super::{set_reconnect_if_required, QueryRaw};

impl QueryRaw {
    pub async fn raw<M, R>(
        &mut self,
        message: M,
        path_and_query: PathAndQuery,
    ) -> Result<R>
    where
        M: Message + 'static,
        R: Message + Default + 'static,
    {
        const CHECK_READY_ERROR: &str =
            "Failed to check if underlying gRPC service channel is ready!";

        const RUN_QUERY_ERROR: &str = "Failed to run raw query!";

        let mut raw_client = self.inner.raw_client().await?;

        raw_client
            .ready()
            .await
            .inspect_err(|_| {
                self.inner.set_should_reconnect();
            })
            .context(CHECK_READY_ERROR)?;

        raw_client
            .unary(
                message.into_request(),
                path_and_query,
                ProstCodec::default(),
            )
            .await
            .map(tonic::Response::into_inner)
            .inspect_err(|status| {
                set_reconnect_if_required(&self.inner, status.code());
            })
            .context(RUN_QUERY_ERROR)
    }
}
