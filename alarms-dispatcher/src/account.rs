use cosmrs::{
    crypto::secp256k1::SigningKey,
    proto::cosmos::auth::v1beta1::{query_client::QueryClient, BaseAccount, QueryAccountRequest},
    AccountId,
};
use prost::Message;

use crate::{
    client::Client,
    configuration::Node,
    context_message,
    error::{ContextError, Error, WithOriginContext},
};

pub fn account_id(
    signing_key: &SigningKey,
    config: &Node,
) -> Result<AccountId, ContextError<Error>> {
    signing_key
        .public_key()
        .account_id(config.address_prefix())
        .map_err(|_| {
            Error::AccountIdDerivationFailed
                .with_origin_context(context_message!("Couldn't derive account ID!"))
        })
}

pub async fn account_data(
    account_id: AccountId,
    client: &Client,
) -> Result<BaseAccount, ContextError<Error>> {
    let account_data = client
        .with_grpc(|grpc| async {
            QueryClient::new(grpc)
                .account(QueryAccountRequest {
                    address: account_id.into(),
                })
                .await
        })
        .await
        .map_err(|error| {
            Error::from(error).with_origin_context(context_message!(
                "Error occurred while fetching account data!"
            ))
        })?
        .into_inner()
        .account
        .ok_or_else(|| {
            Error::AccountNotFound.with_origin_context(context_message!(
                "Account not found! Make sure it's balance is non-zero!"
            ))
        })?;

    Message::decode(account_data.value.as_slice()).map_err(|error| {
        Error::from(error).with_origin_context(context_message!(
            "Account query response's message couldn't be deserialized!"
        ))
    })
}
