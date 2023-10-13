use std::{future::Future, sync::Arc};

use cosmrs::rpc::HttpClient as TendermintRpcClient;
use tonic::transport::{Channel, Endpoint};

use crate::config::Node;

use self::error::Result;

pub mod error;

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct Client(Arc<Inner>);

impl Client {
    pub async fn from_config(config: &Node) -> Result<Self> {
        let json_rpc: TendermintRpcClient = {
            let mut json_rpc: TendermintRpcClient =
                TendermintRpcClient::new(config.json_rpc_url())?;

            json_rpc.set_origin_header(true);

            json_rpc
        };

        let grpc: Channel = {
            let mut channel_builder: Endpoint = Channel::builder(config.grpc_url().try_into()?);

            if let Some(limit) = config.http2_concurrency_limit() {
                channel_builder = channel_builder.concurrency_limit(limit.get());
            }

            channel_builder
                .keep_alive_while_idle(true)
                .connect()
                .await?
        };

        Ok(Self(Arc::new(Inner { json_rpc, grpc })))
    }

    pub async fn with_json_rpc<F, R>(&self, f: F) -> R::Output
    where
        F: FnOnce(TendermintRpcClient) -> R,
        R: Future,
    {
        f(self.0.json_rpc.clone()).await
    }

    pub async fn with_grpc<F, R>(&self, f: F) -> R::Output
    where
        F: FnOnce(Channel) -> R,
        R: Future,
    {
        f(self.0.grpc.clone()).await
    }
}

#[derive(Debug, Clone)]
struct Inner {
    json_rpc: TendermintRpcClient,
    grpc: Channel,
}
