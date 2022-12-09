use cosmrs::{
    crypto::secp256k1::SigningKey,
    proto::cosmos::auth::v1beta1::{query_client::QueryClient, BaseAccount, QueryAccountRequest},
    AccountId,
};
use prost::Message;

use crate::{client::Client, configuration::Node};

use self::error::{AccountData as AccountDataError, AccountDataResult, AccountIdResult};

pub mod error;

pub fn account_id(signing_key: &SigningKey, config: &Node) -> AccountIdResult<AccountId> {
    signing_key
        .public_key()
        .account_id(config.address_prefix())
        .map_err(Into::into)
}

pub async fn account_data(
    account_id: AccountId,
    client: &Client,
) -> AccountDataResult<BaseAccount> {
    let account_data = client
        .with_grpc(|grpc| async {
            QueryClient::new(grpc)
                .account(QueryAccountRequest {
                    address: account_id.into(),
                })
                .await
        })
        .await?
        .into_inner()
        .account
        .ok_or(AccountDataError::NotFound)?;

    Message::decode(account_data.value.as_slice()).map_err(Into::into)
}
