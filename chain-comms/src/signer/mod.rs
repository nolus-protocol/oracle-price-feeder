use cosmrs::{
    crypto::secp256k1::SigningKey,
    proto::{
        cosmos::{
            auth::v1beta1::BaseAccount,
            tx::v1beta1::{SignDoc, TxRaw},
        },
        prost::Message,
    },
    tendermint::chain::Id as ChainId,
    tx::{Body, Fee, Raw, SignerInfo},
};

use crate::{client::Client, interact::query_account_data};

use self::error::Result as ModuleResult;

pub mod error;

pub struct Signer {
    needs_update: bool,
    key: SigningKey,
    chain_id: ChainId,
    account: BaseAccount,
}

impl Signer {
    #[inline]
    #[must_use]
    pub const fn new(key: SigningKey, chain_id: ChainId, account: BaseAccount) -> Self {
        Self {
            needs_update: false,
            key,
            chain_id,
            account,
        }
    }

    #[must_use]
    pub fn signer_address(&self) -> &str {
        &self.account.address
    }

    pub fn sign(&self, body: Body, fee: Fee) -> ModuleResult<Raw> {
        if self.needs_update {
            return Err(error::Error::AccountDataUpdateNeeded);
        }

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
            .map(move |signature| {
                TxRaw {
                    body_bytes: body,
                    auth_info_bytes: auth_info,
                    signatures: vec![signature.to_vec()],
                }
                .into()
            })
            .map_err(Into::into)
    }

    #[inline]
    pub async fn update_account(
        &mut self,
        client: &Client,
    ) -> Result<(), crate::interact::error::AccountQuery> {
        query_account_data(client, &self.account.address)
            .await
            .map(|account: BaseAccount| {
                self.needs_update = false;

                self.account = account;
            })
    }

    #[inline]
    #[must_use]
    pub const fn needs_update(&self) -> bool {
        self.needs_update
    }

    #[inline]
    pub(crate) fn tx_confirmed(&mut self) {
        self.account.sequence += 1;
    }

    #[inline]
    pub(crate) fn set_needs_update(&mut self) {
        self.needs_update = true;
    }
}
