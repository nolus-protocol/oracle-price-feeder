use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{Context as _, Result};
use cosmrs::proto::{
    cosmos::{
        auth::v1beta1::query_client::QueryClient as AuthQueryClient,
        base::{
            reflection::v2alpha1::reflection_service_client::ReflectionServiceClient,
            tendermint::v1beta1::service_client::ServiceClient as TendermintServiceClient,
        },
        tx::v1beta1::service_client::ServiceClient as TxServiceClient,
    },
    cosmwasm::wasm::v1::query_client::QueryClient as WasmQueryClient,
};
use tokio::sync::RwLock;
use tonic::{
    client::Grpc as GrpcClient,
    transport::{Channel as GrpcChannel, Endpoint, Uri},
    Code as TonicCode,
};

mod broadcast_tx;
mod query_auth;
mod query_raw;
mod query_reflection;
mod query_tendermint;
mod query_tx;
mod query_wasm;

pub trait Reconnect {
    fn reconnect(&self) -> impl Future<Output = Result<()>> + Send + '_;
}

#[derive(Clone)]
pub struct Client
where
    Self: Reconnect,
{
    inner: Arc<ClientInner>,
}

impl Client
where
    Self: Reconnect,
{
    pub async fn connect(uri: &str) -> Result<Self> {
        const CONNECT_TO_GRPC_ERROR: &str =
            "Failed to connect to node's gRPC endpoint!";

        let uri: Uri = uri.parse().with_context(|| {
            format!(r#"Failed to parse gRPC URI, "{uri}"!"#)
        })?;

        let endpoint = {
            Endpoint::from(uri.clone())
                .origin(uri.clone())
                .keep_alive_while_idle(true)
        };

        endpoint
            .connect()
            .await
            .map(|grpc| Self {
                inner: Arc::new(ClientInner {
                    should_reconnect: const { AtomicBool::new(false) },
                    uri,
                    endpoint,
                    grpc: RwLock::new(grpc),
                }),
            })
            .context(CONNECT_TO_GRPC_ERROR)
    }
}

impl Reconnect for Client {
    async fn reconnect(&self) -> Result<()> {
        self.inner.reconnect().await
    }
}

macro_rules! define_interface {
    ($($method: ident => $interface: ident),+ $(,)?) => {
        $(
            #[derive(Clone)]
            #[must_use]
            pub struct $interface
            where
                Self: Reconnect,
            {
                inner: Arc<ClientInner>,
            }

            impl $interface
            where
                Self: Reconnect,
            {
                #[inline]
                const fn new(inner: Arc<ClientInner>) -> Self {
                    Self { inner }
                }
            }

            impl Reconnect for $interface {
                async fn reconnect(&self) -> Result<()> {
                    self.inner.reconnect().await
                }
            }

            impl Client
            where
                Self: Reconnect,
            {
                #[inline]
                pub fn $method(self) -> $interface
                where
                    $interface: Reconnect,
                {
                    $interface::new(self.inner)
                }
            }
        )+
    };
}

define_interface![
    broadcast_tx => BroadcastTx,
    query_auth => QueryAuth,
    query_raw => QueryRaw,
    query_reflection => QueryReflection,
    query_tendermint => QueryTendermint,
    query_tx => QueryTx,
    query_wasm => QueryWasm,
];

struct ClientInner {
    should_reconnect: AtomicBool,
    uri: Uri,
    endpoint: Endpoint,
    grpc: RwLock<GrpcChannel>,
}

impl ClientInner {
    fn set_should_reconnect(&self) {
        self.should_reconnect.store(true, Ordering::Release);
    }

    async fn reconnect_if_required(&self) -> Result<()> {
        if self.should_reconnect.load(Ordering::Acquire) {
            self.reconnect().await
        } else {
            Ok(())
        }
    }

    async fn auth_query_client(
        self: &Arc<Self>,
    ) -> Result<AuthQueryClient<GrpcChannel>> {
        self.reconnect_if_required().await?;

        Ok(AuthQueryClient::with_origin(
            self.grpc.read().await.clone(),
            self.uri.clone(),
        ))
    }

    async fn tendermint_service_client(
        self: &Arc<Self>,
    ) -> Result<TendermintServiceClient<GrpcChannel>> {
        self.reconnect_if_required().await?;

        Ok(TendermintServiceClient::with_origin(
            self.grpc.read().await.clone(),
            self.uri.clone(),
        ))
    }

    async fn tx_service_client(
        self: &Arc<Self>,
    ) -> Result<TxServiceClient<GrpcChannel>> {
        self.reconnect_if_required().await?;

        Ok(TxServiceClient::with_origin(
            self.grpc.read().await.clone(),
            self.uri.clone(),
        ))
    }

    async fn raw_client(self: &Arc<Self>) -> Result<GrpcClient<GrpcChannel>> {
        self.reconnect_if_required().await?;

        Ok(GrpcClient::new(self.grpc.read().await.clone()))
    }

    async fn reflection_service_client(
        self: &Arc<Self>,
    ) -> Result<ReflectionServiceClient<GrpcChannel>> {
        self.reconnect_if_required().await?;

        Ok(ReflectionServiceClient::with_origin(
            self.grpc.read().await.clone(),
            self.uri.clone(),
        ))
    }

    async fn wasm_query_client(
        self: &Arc<Self>,
    ) -> Result<WasmQueryClient<GrpcChannel>> {
        self.reconnect_if_required().await?;

        Ok(WasmQueryClient::with_origin(
            self.grpc.read().await.clone(),
            self.uri.clone(),
        ))
    }
}

impl Reconnect for ClientInner {
    async fn reconnect(&self) -> Result<()> {
        const RECONNECT_ERROR: &str =
            "Failed to reconnect to node's gRPC endpoint!";

        let mut lock = self.grpc.write().await;

        if self.should_reconnect.load(Ordering::Acquire) {
            let new_channel =
                self.endpoint.connect().await.context(RECONNECT_ERROR)?;

            *lock = new_channel;

            self.should_reconnect.store(false, Ordering::Release);
        }

        Ok(())
    }
}

fn set_reconnect_if_required(
    client_inner: &ClientInner,
    error_code: TonicCode,
) {
    if matches!(error_code, TonicCode::Ok | TonicCode::NotFound) {
        client_inner.set_should_reconnect();
    }
}
