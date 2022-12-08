use cosmrs::{
    crypto::secp256k1::SigningKey,
    proto::cosmos::{
        auth::v1beta1::BaseAccount,
        tx::v1beta1::{SignDoc, TxRaw},
    },
    tendermint::chain::Id as ChainId,
    tx::{Body, Fee, Raw, SignerInfo},
};
use prost::Message;

use crate::{
    context_message,
    error::{ContextError, Error, WithOriginContext},
};

pub struct Signer {
    address: String,
    key: SigningKey,
    chain_id: ChainId,
    account: BaseAccount,
}

impl Signer {
    #[inline]
    #[must_use]
    pub fn new(address: String, key: SigningKey, chain_id: ChainId, account: BaseAccount) -> Self {
        Self {
            address,
            key,
            chain_id,
            account,
        }
    }

    pub fn signer_address(&self) -> &str {
        &self.address
    }

    pub fn sign(&self, body: Body, fee: Fee) -> Result<Raw, ContextError<Error>> {
        let body = Message::encode_to_vec(&body.into_proto());

        let auth_info = Message::encode_to_vec(
            &SignerInfo::single_direct(Some(self.key.public_key()), self.account.sequence)
                .auth_info(fee)
                .into_proto(),
        );

        self.key
            .sign(
                Message::encode_to_vec(&SignDoc {
                    body_bytes: body.clone(),
                    auth_info_bytes: auth_info.clone(),
                    chain_id: self.chain_id.to_string(),
                    account_number: self.account.account_number,
                })
                .as_slice(),
            )
            .map_err(|error| {
                Error::Signing(error)
                    .with_origin_context(context_message!("Signing transaction failed!"))
            })
            .map(move |signature| {
                TxRaw {
                    body_bytes: body,
                    auth_info_bytes: auth_info,
                    signatures: vec![signature.to_vec()],
                }
                .into()
            })
    }

    #[inline]
    pub fn tx_confirmed(&mut self) {
        self.account.sequence += 1;
    }

    #[inline]
    pub fn update_account(&mut self, account: BaseAccount) {
        self.account = account;
    }
}
