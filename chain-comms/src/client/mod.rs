use std::future::Future;

use cosmrs::rpc::HttpClient as TendermintRpcClient;
use tonic::transport::Channel;

use crate::config::{Node, Protocol};

use self::error::Result;

pub mod error;

#[derive(Debug, Clone)]
pub struct Client {
    json_rpc: TendermintRpcClient,
    grpc: Channel,
}

impl Client {
    pub async fn new(config: &Node) -> Result<Self> {
        let json_rpc = TendermintRpcClient::new(Self::construct_json_rpc_url(config).as_str())?;

        let grpc = Channel::builder(Self::construct_grpc_url(config).try_into()?)
            .connect()
            .await?;

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

    fn construct_json_rpc_url(config: &Node) -> String {
        Self::construct_url(
            config.json_rpc_protocol(),
            config.host(),
            config.json_rpc_port(),
            config.json_rpc_api_path(),
        )
    }

    fn construct_grpc_url(config: &Node) -> String {
        Self::construct_url(
            config.grpc_protocol(),
            config.host(),
            config.grpc_port(),
            config.grpc_api_path(),
        )
    }

    fn construct_url(protocol: Protocol, host: &str, port: u16, path: Option<&str>) -> String {
        format!(
            "http{}://{}:{}/{}",
            match protocol {
                Protocol::Http => "",
                Protocol::Https => "s",
            },
            host,
            port,
            if let Some(path) = path {
                if let Some("/") = path.get(..1) {
                    &path[1..]
                } else {
                    path
                }
            } else {
                ""
            },
        )
    }
}
