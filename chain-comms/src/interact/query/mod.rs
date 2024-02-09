use cosmrs::proto::{
    cosmos::auth::v1beta1::{BaseAccount, QueryAccountRequest, QueryAccountResponse},
    cosmwasm::wasm::v1::{
        query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
    },
    prost::Message,
};
use serde::de::DeserializeOwned;
use tonic::codec::ProstCodec;
use tonic::{
    client::Grpc as GrpcClient, codegen::http::uri::PathAndQuery,
    transport::Channel as TonicChannel, IntoRequest as _, Response as TonicResponse,
};
use tracing::debug;

use crate::client::Client;

use self::error::{AccountData as AccountError, Raw as RawError, Wasm as WasmError};

pub mod error;

pub async fn account_data(client: &Client, address: &str) -> Result<BaseAccount, AccountError> {
    BaseAccount::decode(
        {
            let data = client
                .auth_query_client()
                .account(QueryAccountRequest {
                    address: address.into(),
                })
                .await
                .map(TonicResponse::into_inner)
                .map_err(AccountError::Rpc)
                .and_then(|QueryAccountResponse { account }| {
                    account.ok_or(AccountError::NoAccountData)
                })
                .map(|account| account.value)?;

            debug!("gRPC query response from {address} returned successfully!");

            data
        }
        .as_slice(),
    )
    .map_err(Into::into)
}

pub async fn raw<Q, R>(rpc: TonicChannel, query: Q, type_url: &'static str) -> Result<R, RawError>
where
    Q: Message + 'static,
    R: Message + Default + 'static,
{
    let mut grpc_client: GrpcClient<TonicChannel> = GrpcClient::new(rpc.clone());

    grpc_client.ready().await?;

    grpc_client
        .unary(
            query.into_request(),
            PathAndQuery::from_static(type_url),
            ProstCodec::default(),
        )
        .await
        .map(tonic::Response::into_inner)
        .map_err(RawError::Response)
}

pub async fn wasm_smart<R>(
    query_client: &mut WasmQueryClient<TonicChannel>,
    address: String,
    query: &[u8],
) -> Result<R, WasmError>
where
    R: DeserializeOwned,
{
    query_client
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
