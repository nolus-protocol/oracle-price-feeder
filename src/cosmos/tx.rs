use cosmos_sdk_proto::cosmos::tx::v1beta1::TxRaw;
use cosmrs::{
    tendermint::{self, chain::Id},
    tx::{Body, Fee, Raw, SignDoc, SignerInfo},
    Coin, Denom,
};

use super::{error::TxBuildError, wallet::Wallet};

/// AccountInfo is a private structure which represents the information of the account
/// that is performing the transaction.
struct AccountInfo {
    pub sequence: u64,
    pub number: u64,
}

/// TxBuilder represents the single signer transaction builder.
pub struct TxBuilder {
    chain_id: Id,
    account_info: Option<AccountInfo>,
    tx_body: Body,
    fee: Option<Fee>,
}

impl TxBuilder {
    pub fn new(chain_id: String) -> Result<TxBuilder, TxBuildError> {
        Ok(Self {
            chain_id: chain_id.parse::<tendermint::chain::Id>()?,
            account_info: None,
            tx_body: Body::new(vec![], "", 0u32),
            fee: None,
        })
    }

    /// Sets the transaction timout height.
    pub fn timeout_height(mut self, timeout_height: u32) -> Self {
        self.tx_body.timeout_height = timeout_height.into();
        self
    }

    /// Sets the transaction memo.
    pub fn memo(mut self, memo: &str) -> Self {
        self.tx_body.memo = memo.to_string();
        self
    }

    /// Sets the transaction fee.
    pub fn fee(
        mut self,
        fee_denom: String,
        funds_amount: u32,
        gas_limit: u64,
    ) -> Result<Self, TxBuildError> {
        let coin = Coin {
            denom: fee_denom.parse::<Denom>()?,
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
        let account_info = match self.account_info {
            Some(info) => info,
            None => return Result::Err(TxBuildError::NoAccountInfo),
        };
        let fee = match self.fee {
            Some(fee) => fee,
            None => return Result::Err(TxBuildError::NoFee),
        };
        let auth_info =
            SignerInfo::single_direct(Some(wallet.get_public_key()), account_info.sequence)
                .auth_info(fee);
        let sign_doc = SignDoc::new(
            &self.tx_body,
            &auth_info,
            &self.chain_id,
            account_info.number,
        )?;
        let sign_doc_bytes = sign_doc.into_bytes()?;
        let signature = wallet.sign(&sign_doc_bytes)?;

        Ok(TxRaw {
            body_bytes: self.tx_body.into_bytes()?,
            auth_info_bytes: auth_info.into_bytes()?,
            signatures: vec![signature],
        }
        .into())
    }
}
