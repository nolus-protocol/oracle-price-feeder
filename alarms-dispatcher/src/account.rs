use cosmrs::{
    crypto::secp256k1::SigningKey,
    proto::cosmos::auth::v1beta1::{query_client::QueryClient, BaseAccount, QueryAccountRequest},
    AccountId,
};
use prost::Message;

use crate::{client::Client, configuration::Node, error::Error, log_error};

pub fn account_id(signing_key: &SigningKey, config: &Node) -> Result<AccountId, Error> {
    log_error!(
        signing_key.public_key().account_id(config.address_prefix()),
        "Couldn't derive account ID!"
    )
    .map_err(|_| Error::AccountIdDerivationFailed)
}

pub async fn account_data(account_id: AccountId, client: &Client) -> Result<BaseAccount, Error> {
    let account_data = log_error!(
        log_error!(
            client
                .with_grpc(|grpc| async {
                    QueryClient::new(grpc)
                        .account(QueryAccountRequest {
                            address: account_id.into(),
                        })
                        .await
                })
                .await,
            "Error occurred while fetching account data!"
        )?
        .into_inner()
        .account
        .ok_or(Error::AccountNotFound),
        "Account not found! Make sure it's balance is non-zero!"
    )?;

    log_error!(
        Message::decode(account_data.value.as_slice()),
        "Account query response's message couldn't be deserialized!"
    )
    .map_err(Into::into)
}
