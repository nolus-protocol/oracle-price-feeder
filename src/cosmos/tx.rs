use cosmos_sdk_proto::cosmos::tx::v1beta1::TxRaw;
use cosmrs::{
    tendermint::chain::Id,
    tx::{Body, Fee, Raw, SignDoc, SignerInfo},
    Coin,
};

use super::{error::TxBuild as TxBuildError, wallet::Wallet};

/// [`AccountInfo`] is a private structure which represents the information of the account
/// that is performing the transaction.
struct AccountInfo {
    pub sequence: u64,
    pub number: u64,
}

/// [`TxBuilder`](Self) represents the single signer transaction builder.
#[must_use]
pub struct Builder {
    chain_id: Id,
    account_info: Option<AccountInfo>,
    tx_body: Body,
    fee: Option<Fee>,
}

impl Builder {
    pub fn new(chain_id: &str) -> Result<Builder, TxBuildError> {
        Ok(Self {
            chain_id: chain_id.parse()?,
            account_info: None,
            tx_body: Body::default(),
            fee: None,
        })
    }

    /// Sets the transaction timout height.
    pub fn timeout_height(mut self, timeout_height: u32) -> Self {
        self.tx_body.timeout_height = timeout_height.into();

        self
    }

    /// Sets the transaction memo.
    pub fn memo(mut self, memo: String) -> Self {
        self.tx_body.memo = memo;

        self
    }

    /// Sets the transaction fee.
    pub fn fee(
        mut self,
        fee_denom: &str,
        funds_amount: u32,
        gas_limit: u64,
    ) -> Result<Self, TxBuildError> {
        let coin = Coin {
            denom: fee_denom.parse()?,
            amount: funds_amount.into(),
        };

        self.fee = Some(Fee::from_amount_and_gas(coin, gas_limit));

        Ok(self)
    }

    /// Sets the account information.
    pub fn account_info(mut self, sequence: u64, number: u64) -> Self {
        self.account_info = Some(AccountInfo { sequence, number });

        self
    }

    /// Append a message to the transaction messages.
    pub fn add_message(mut self, msg: cosmrs::Any) -> Self {
        self.tx_body.messages.push(msg);

        self
    }

    pub fn sign(self, wallet: &Wallet) -> Result<Raw, TxBuildError> {
        let account_info = self.account_info.ok_or(TxBuildError::NoAccountInfo)?;

        let fee = self.fee.ok_or(TxBuildError::NoFee)?;

        let auth_info =
            SignerInfo::single_direct(Some(wallet.get_public_key()), account_info.sequence)
                .auth_info(fee);

        let signature;

        {
            let sign_doc_bytes = SignDoc::new(
                &self.tx_body,
                &auth_info,
                &self.chain_id,
                account_info.number,
            )?
            .into_bytes()?;

            signature = wallet.sign(&sign_doc_bytes)?;
        }

        Ok(TxRaw {
            body_bytes: self.tx_body.into_bytes()?,
            auth_info_bytes: auth_info.into_bytes()?,
            signatures: vec![signature],
        }
        .into())
    }
}
