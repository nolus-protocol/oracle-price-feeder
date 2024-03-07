use std::{num::NonZeroUsize, sync::Arc};

use cosmrs::proto::{
    cosmos::{
        auth::v1beta1::query_client::QueryClient as AuthQueryClient,
        base::tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient,
        tx::v1beta1::service_client::ServiceClient as TxServiceClient,
    },
    cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient,
};
use tonic::transport::{Channel as GrpcChannel, Uri};

use crate::config::Node;

use self::error::Result;

pub mod error;

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct Client(Arc<GrpcChannel>);

impl Client {
    pub async fn new(grpc_uri: &str, concurrency_limit: Option<NonZeroUsize>) -> Result<Self> {
        let grpc: GrpcChannel = {
            let grpc_uri: Uri = grpc_uri.try_into()?;

            let origin: Uri = format!(
                "{}://{}:{}",
                grpc_uri
                    .scheme_str()
                    .ok_or(error::Error::GrpcUriNoSchemeSet)?,
                grpc_uri.host().ok_or(error::Error::GrpcUriNoHostSet)?,
                grpc_uri.port_u16().map_or_else(
                    || grpc_uri
                        .scheme_str()
                        .and_then(|scheme| match scheme {
                            "http" | "ws" => Some(80),
                            "https" | "wss" => Some(443),
                            _ => None,
                        })
                        .ok_or(error::Error::GrpcUriNoPortSet),
                    Ok,
                )?,
            )
            .try_into()?;

            GrpcChannel::builder(grpc_uri)
                .origin(origin)
                .pipe_if_some(
                    |_| concurrency_limit,
                    |endpoint, limit| endpoint.concurrency_limit(limit.get()),
                )
                .keep_alive_while_idle(true)
                .connect()
                .await?
        };

        Ok(Self(Arc::new(grpc)))
    }

    pub async fn from_config(config: &Node) -> Result<Self> {
        Self::new(config.grpc_uri(), config.http2_concurrency_limit()).await
    }

    #[must_use]
    pub fn raw_grpc(&self) -> GrpcChannel {
        GrpcChannel::clone(&self.0)
    }

    #[must_use]
    pub fn auth_query_client(&self) -> AuthQueryClient<GrpcChannel> {
        AuthQueryClient::new(self.raw_grpc())
    }

    #[must_use]
    pub fn tendermint_service_client(&self) -> TendermintServiceClient<GrpcChannel> {
        TendermintServiceClient::new(self.raw_grpc())
    }

    #[must_use]
    pub fn tx_service_client(&self) -> TxServiceClient<GrpcChannel> {
        TxServiceClient::new(self.raw_grpc())
    }

    #[must_use]
    pub fn wasm_query_client(&self) -> WasmQueryClient<GrpcChannel> {
        WasmQueryClient::new(self.raw_grpc())
    }
}

trait PipeIf: Sized {
    fn pipe_if_some<T, TryF, MapF>(self, try_f: TryF, map_f: MapF) -> Self
    where
        TryF: FnOnce(&Self) -> Option<T>,
        MapF: FnOnce(Self, T) -> Self;
}

impl<T> PipeIf for T {
    fn pipe_if_some<U, TryF, MapF>(self, try_f: TryF, map_f: MapF) -> Self
    where
        TryF: FnOnce(&Self) -> Option<U>,
        MapF: FnOnce(Self, U) -> Self,
    {
        if let Some(input) = try_f(&self) {
            map_f(self, input)
        } else {
            self
        }
    }
}
