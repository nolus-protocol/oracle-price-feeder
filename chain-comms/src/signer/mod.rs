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
    tx::{Body, Fee, SignerInfo},
    AccountId,
};

use crate::{client::Client as NodeClient, interact::query};

use self::error::Result as ModuleResult;

pub mod error;

pub struct Signer {
    key: SigningKey,
    chain_id: ChainId,
    account_id: AccountId,
    account: BaseAccount,
}

impl Signer {
    #[inline]
    #[must_use]
    pub const fn new(
        key: SigningKey,
        chain_id: ChainId,
        account_id: AccountId,
        account: BaseAccount,
    ) -> Self {
        Self {
            key,
            chain_id,
            account_id,
            account,
        }
    }

    #[must_use]
    pub fn signer_address(&self) -> &str {
        &self.account.address
    }

    pub fn sign(&mut self, body: Body, fee: Fee) -> ModuleResult<TxRaw> {
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
            .map(move |signature| TxRaw {
                body_bytes: body,
                auth_info_bytes: auth_info,
                signatures: vec![signature.to_vec()],
            })
            .map_err(Into::into)
    }

    #[inline]
    pub async fn fetch_sequence_number(
        &mut self,
        node_client: &NodeClient,
    ) -> Result<(), query::error::AccountData> {
        query::account_data(node_client, &self.account_id)
            .await
            .map(|account_data| self.account = account_data)
    }

    #[inline]
    pub(crate) fn tx_confirmed(&mut self) {
        self.account.sequence += 1;
    }
}
