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

use crate::configuration::Oracle;

use super::{error::CosmosError, QueryMsg};

/// Client to communicate with a full node.
#[derive(Clone)]
pub struct CosmosClient {
    config: Oracle,
    grpc_channel: Endpoint,
}

impl CosmosClient {
    pub fn new(config: Oracle) -> Result<CosmosClient, InvalidUri> {
        let grpc_uri = format!("{}:{}", config.host_url, config.grpc_port).parse::<Uri>()?;
        let grpc_channel = Channel::builder(grpc_uri);

        Ok(CosmosClient {
            grpc_channel,
            config,
        })
    }

    /// Returns the account data associated to the given address.
    pub async fn get_account_data(&self, address: &str) -> Result<BaseAccount, CosmosError> {
        // Create channel connection to the gRPC server
        let channel = self
            .grpc_channel
            .connect()
            .await
            .map_err(|err| CosmosError::Grpc(err.to_string()))?;

        // Create gRPC query auth client from channel
        let mut client = QueryClient::new(channel);

        // Build a new request
        let request = Request::new(QueryAccountRequest {
            address: ToOwned::to_owned(address),
        });

        // Send request and wait for response
        let response = client
            .account(request)
            .await
            .map_err(|err| CosmosError::Grpc(err.to_string()))?
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
        let channel = self
            .grpc_channel
            .connect()
            .await
            .map_err(|err| CosmosError::Grpc(err.to_string()))?;

        // Create gRPC query auth client from channel
        let mut client = WasmQueryClient::new(channel);

        // Send request and wait for response
        let response = client
            .smart_contract_state(QuerySmartContractStateRequest {
                address: self.config.contract_addrs.clone(),
                query_data: serde_json::to_vec(msg)?,
            })
            .await
            .map_err(|err| CosmosError::Grpc(err.to_string()))?
            .into_inner();

        Ok(response)
    }
}
