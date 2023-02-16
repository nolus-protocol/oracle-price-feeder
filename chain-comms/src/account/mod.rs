use cosmrs::{crypto::secp256k1::SigningKey, AccountId};

use crate::config::Node;

use self::error::AccountIdResult;

pub mod error;

#[cold]
#[inline]
pub fn account_id(config: &Node, signing_key: &SigningKey) -> AccountIdResult<AccountId> {
    signing_key
        .public_key()
        .account_id(config.address_prefix())
        .map_err(Into::into)
}
