use std::future::Future;

use cosmrs::rpc::HttpClient as TendermintRpcClient;
use tonic::transport::Channel;

use crate::config::Node;

use self::error::Result;

pub mod error;

#[derive(Debug, Clone)]
pub struct Client {
    json_rpc: TendermintRpcClient,
    grpc: Channel,
}

impl Client {
    pub async fn new(config: &Node) -> Result<Self> {
        let json_rpc = TendermintRpcClient::new(config.json_rpc_url())?;

        let grpc = {
            let mut channel_builder = Channel::builder(config.grpc_url().try_into()?);

            if let Some(limit) = config.http2_concurrency_limit() {
                channel_builder = channel_builder.concurrency_limit(limit.get());
            }

            channel_builder
                .keep_alive_while_idle(true)
                .connect()
                .await?
        };

        Ok(Self { json_rpc, grpc })
    }

    pub async fn with_json_rpc<F, R>(&self, f: F) -> R::Output
    where
        F: FnOnce(TendermintRpcClient) -> R,
        R: Future,
    {
        f(self.json_rpc.clone()).await
    }

    pub async fn with_grpc<F, R>(&self, f: F) -> R::Output
    where
        F: FnOnce(Channel) -> R,
        R: Future,
    {
        f(self.grpc.clone()).await
    }
}
