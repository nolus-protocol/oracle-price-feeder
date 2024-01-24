use cosmrs::proto::{
    cosmos::auth::v1beta1::{
        BaseAccount, query_client::QueryClient as AuthQueryClient, QueryAccountRequest,
    },
    cosmwasm::wasm::v1::{
        query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
    },
    prost,
};
use serde::de::DeserializeOwned;
use tonic::{client::Grpc as GrpcClient, IntoRequest as _, transport::Channel as TonicChannel};
use tracing::debug;

use crate::client::Client;

use self::error::{AccountData as AccountError, Raw as RawError, Wasm as WasmError};

pub mod error;

pub async fn account_data(client: &Client, address: &str) -> Result<BaseAccount, AccountError> {
    prost::Message::decode(
        {
            let data = client
                .with_grpc(move |rpc: TonicChannel| async move {
                    AuthQueryClient::new(rpc)
                        .account(QueryAccountRequest {
                            address: address.into(),
                        })
                        .await
                })
                .await?
                .into_inner()
                .account
                .ok_or(AccountError::NoAccountData)?
                .value;

            debug!("gRPC query response from {address} returned successfully!");

            data
        }
        .as_slice(),
    )
    .map_err(Into::into)
}

pub async fn raw<Q, R>(rpc: TonicChannel, query: Q, type_url: &'static str) -> Result<R, RawError>
where
    Q: prost::Message + 'static,
    R: prost::Message + Default + 'static,
{
    let mut grpc_client: GrpcClient<TonicChannel> = GrpcClient::new(rpc.clone());

    grpc_client.ready().await?;

    grpc_client
        .unary(
            query.into_request(),
            http::uri::PathAndQuery::from_static(type_url),
            tonic::codec::ProstCodec::default(),
        )
        .await
        .map(tonic::Response::into_inner)
        .map_err(RawError::Response)
}

pub async fn wasm<R>(rpc: TonicChannel, address: String, query: &[u8]) -> Result<R, WasmError>
where
    R: DeserializeOwned,
{
    WasmQueryClient::new(rpc)
        .smart_contract_state(QuerySmartContractStateRequest {
            address,
            query_data: query.to_vec(),
        })
        .await
        .map_err(|error| WasmError::RawQuery(RawError::Response(error)))
        .and_then(|response| {
            serde_json_wasm::from_slice(&response.into_inner().data).map_err(From::from)
        })
}
