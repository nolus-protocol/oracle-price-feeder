use cosmos_sdk_proto::{
    cosmos::auth::v1beta1::{query_client::QueryClient, BaseAccount, QueryAccountRequest},
    cosmwasm::wasm::v1::{
        query_client::QueryClient as WasmQueryClient, QuerySmartContractStateRequest,
        QuerySmartContractStateResponse,
    },
};
use prost::Message;
use tonic::{
    codegen::http::uri::InvalidUri,
    transport::{Channel, Endpoint, Uri},
    Request,
};
use tracing::{error, trace};

use crate::configuration::Oracle;

use super::{error::Cosmos as CosmosError, QueryMsg};

/// Client to communicate with a full node.
#[derive(Clone)]
pub struct Client {
    config: Oracle,
    grpc_channel: Endpoint,
}

impl Client {
    pub fn new(config: Oracle) -> Result<Client, InvalidUri> {
        let grpc_uri = format!("{}:{}", config.host_url, config.grpc_port).parse::<Uri>()?;
        let grpc_channel = Channel::builder(grpc_uri);

        Ok(Client {
            config,
            grpc_channel,
        })
    }

    /// Returns the account data associated to the given address.
    pub async fn get_account_data(&self, address: &str) -> Result<BaseAccount, CosmosError> {
        trace!("Creating gRPC channel.");

        // Create channel connection to the gRPC server
        let channel = self.grpc_channel.connect().await.map_err(|error| {
            error!(
                error = ?error,
                "Error occurred when connecting to gRPC channel."
            );

            CosmosError::GrpcTransport(error)
        })?;

        // Create gRPC query auth client from channel
        let mut client = QueryClient::new(channel);

        // Build a new request
        let request = Request::new(QueryAccountRequest {
            address: ToOwned::to_owned(address),
        });

        trace!("Sending account query request through gRPC channel.");

        // Send request and wait for response
        let response = client
            .account(request)
            .await
            .map_err(|error| {
                error!(
                    error = ?error,
                    "Error occurred while querying account data!"
                );

                CosmosError::GrpcResponse(error)
            })?
            .into_inner();

        match response.account {
            Some(account) => {
                // Decode response body into BaseAccount
                let base_account: BaseAccount = Message::decode(account.value.as_slice())?;

                Ok(base_account)
            }
            None => Err(CosmosError::AccountNotFound(ToOwned::to_owned(address))),
        }
    }

    /// Returns the account data associated to the given address.
    pub async fn cosmwasm_query(
        &self,
        msg: &QueryMsg,
    ) -> Result<QuerySmartContractStateResponse, CosmosError> {
        // Create channel connection to the gRPC server
        let channel = self.grpc_channel.connect().await.map_err(|error| {
            error!(
                error = ?error,
                "Error occurred while connecting to gRPC channel!"
            );

            CosmosError::GrpcTransport(error)
        })?;

        // Create gRPC query auth client from channel
        let mut client = WasmQueryClient::new(channel);

        // Send request and wait for response
        let response = client
            .smart_contract_state(QuerySmartContractStateRequest {
                address: self.config.contract_addrs.clone(),
                query_data: serde_json::to_vec(msg)?,
            })
            .await
            .map_err(|error| {
                error!(
                    error = ?error,
                    "Error occurred while connecting to gRPC channel!"
                );

                CosmosError::GrpcResponse(error)
            })?
            .into_inner();

        Ok(response)
    }
}
