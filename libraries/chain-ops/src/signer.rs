use std::{
    borrow::Borrow,
    num::NonZeroU32,
    ops::{Div, Mul},
    sync::Arc,
};

use anyhow::{anyhow, Context as _, Result};
pub use cosmrs::Gas;
use cosmrs::{
    auth::BaseAccount,
    tendermint::chain::Id as ChainId,
    tx::{
        AccountNumber, Body as TxBody, Fee, Raw, SequenceNumber, SignDoc,
        SignerInfo,
    },
    AccountId, Amount, Coin,
};

use environment::ReadFromVar;

use crate::{
    key::{Public as PublicKey, Signing as SigningKey},
    node,
};

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "signer",
            $($body)+
        );
    };
}

#[derive(Clone)]
#[must_use]
pub struct Signer {
    query_auth: node::QueryAuth,
    sequence_number: SequenceNumber,
    immutable: Arc<Immutable>,
}

impl Signer {
    pub async fn new(
        node_client: node::Client,
        signing_key: SigningKey,
        fee_token: String,
        gas_and_fee_configuration: GasAndFeeConfiguration,
    ) -> Result<Self> {
        let chain_id = node_client
            .clone()
            .query_tendermint()
            .chain_id()
            .await
            .context("Failed to fetch network's chain ID!")?;

        let public_key = signing_key.public_key();

        let account_id = public_key
            .account_id(
                &node_client
                    .clone()
                    .query_reflection()
                    .account_prefix()
                    .await
                    .context("Failed to fetch account prefix!")?,
            )
            .map_err(|error| anyhow!(error))
            .context("Failed to derive account ID!")?;

        let mut query_auth = node_client.query_auth();

        let BaseAccount {
            account_number,
            sequence: sequence_number,
            ..
        } = query_auth
            .account(account_id.to_string())
            .await
            .context("Failed to query account information!")?;

        Ok(Self {
            query_auth,
            sequence_number,
            immutable: Arc::new(Immutable {
                signing_key,
                public_key,
                account_id,
                account_number,
                fee_token,
                gas_and_fee_configuration,
                chain_id,
            }),
        })
    }

    #[must_use]
    #[inline]
    pub fn address(&self) -> &str {
        self.immutable.account_id.as_ref()
    }

    #[must_use]
    #[inline]
    pub fn fee_token(&self) -> &str {
        &self.immutable.fee_token
    }

    pub fn tx(&self, body: &TxBody, gas_limit: Gas) -> Result<Raw> {
        SignDoc::new(
            body,
            &SignerInfo::single_direct(
                Some(self.immutable.public_key),
                self.sequence_number,
            )
            .auth_info(Fee::from_amount_and_gas(
                Coin::new(
                    self.immutable
                        .gas_and_fee_configuration
                        .calculate_fee(gas_limit),
                    &self.immutable.fee_token,
                )
                .map_err(|error| anyhow!(error))
                .context("Failed to construct `cosmrs`'s `Coin` structure!")?,
                gas_limit,
            )),
            &self.immutable.chain_id,
            self.immutable.account_number,
        )
        .map_err(|error| anyhow!(error))
        .context("Failed to construct `cosmrs`'s `SignDoc` structure!")?
        .sign(&self.immutable.signing_key)
        .map_err(|error| anyhow!(error))
        .context("Failed to sign transaction document!")
    }

    pub fn tx_with_gas_adjustment(
        &self,
        body: &TxBody,
        required_gas: Gas,
        hard_gas_limit: Gas,
    ) -> Result<Raw> {
        self.immutable
            .gas_and_fee_configuration
            .calculate_gas(required_gas)
            .map(|gas| {
                if gas <= hard_gas_limit {
                    gas
                } else {
                    log!(warn!(
                        "Gas after adjustment exceeds hard gas limit. Clamping \
                        down.",
                    ));

                    hard_gas_limit
                }
            })
            .context("Failed to calculate adjusted gas limit!")
            .and_then(|gas_limit| {
                self.tx(body, gas_limit)
                    .context("Failed to construct the transaction object!")
            })
    }

    #[must_use]
    #[inline]
    pub const fn sequence_number(&self) -> SequenceNumber {
        self.sequence_number
    }

    pub async fn fetch_sequence_number(&mut self) -> Result<()> {
        self.query_auth
            .account(self.immutable.account_id.to_string())
            .await
            .map(|BaseAccount { sequence, .. }| self.sequence_number = sequence)
            .context("Failed to fetch sequence number!")
    }

    #[inline]
    pub fn increment_sequence_number(&mut self) {
        self.sequence_number += 1;
    }
}

#[must_use]
pub struct GasAndFeeConfiguration {
    pub gas_adjustment_numerator: u32,
    pub gas_adjustment_denominator: NonZeroU32,
    pub gas_price_numerator: u32,
    pub gas_price_denominator: NonZeroU32,
    pub fee_adjustment_numerator: u32,
    pub fee_adjustment_denominator: NonZeroU32,
}

impl GasAndFeeConfiguration {
    fn calculate_gas(&self, gas_limit: Gas) -> Result<Gas>
    where
        Gas: Into<u128>,
        u128: TryInto<Gas>,
    {
        ((u128::from(gas_limit) * u128::from(self.gas_adjustment_numerator))
            / u128::from(self.gas_adjustment_denominator.get()))
        .try_into()
        .context("Failed to convert back to gas due to an integer overflow!")
    }

    fn calculate_fee(&self, gas_limit: Gas) -> Amount
    where
        Gas: Into<Amount>,
        u32: Into<Amount>,
        Amount: Mul<Amount, Output = Amount>,
        Amount: Div<Amount, Output = Amount>,
    {
        (Amount::from(gas_limit)
            * Amount::from(self.gas_price_numerator)
            * Amount::from(self.fee_adjustment_numerator))
            / (Amount::from(self.gas_price_denominator.get())
                * Amount::from(self.fee_adjustment_denominator.get()))
    }
}

impl ReadFromVar for GasAndFeeConfiguration {
    fn read_from_var<S>(variable: S) -> Result<Self>
    where
        S: Borrow<str> + Into<String>,
    {
        let mut variable = variable.into();

        if !variable.is_empty() {
            variable.push_str("__");
        }

        let gas_adjustment_numerator = {
            let mut variable = variable.clone();

            variable.push_str("GAS_ADJUSTMENT_NUMERATOR");

            u32::read_from_var(variable)
                .context("Failed to read gas adjustment numerator!")?
        };

        let gas_adjustment_denominator = {
            let mut variable = variable.clone();

            variable.push_str("GAS_ADJUSTMENT_DENOMINATOR");

            NonZeroU32::read_from_var(variable)
                .context("Failed to read gas adjustment denominator!")?
        };

        let gas_price_numerator = {
            let mut variable = variable.clone();

            variable.push_str("GAS_PRICE_NUMERATOR");

            u32::read_from_var(variable)
                .context("Failed to read gas price numerator!")?
        };

        let gas_price_denominator = {
            let mut variable = variable.clone();

            variable.push_str("GAS_PRICE_DENOMINATOR");

            NonZeroU32::read_from_var(variable)
                .context("Failed to read gas price denominator!")?
        };

        let fee_adjustment_numerator = {
            let mut variable = variable.clone();

            variable.push_str("FEE_ADJUSTMENT_NUMERATOR");

            u32::read_from_var(variable)
                .context("Failed to read fee adjustment numerator!")?
        };

        let fee_adjustment_denominator = {
            variable.push_str("FEE_ADJUSTMENT_DENOMINATOR");

            NonZeroU32::read_from_var(variable)
                .context("Failed to read fee adjustment denominator!")?
        };

        Ok(GasAndFeeConfiguration {
            gas_adjustment_numerator,
            gas_adjustment_denominator,
            gas_price_numerator,
            gas_price_denominator,
            fee_adjustment_numerator,
            fee_adjustment_denominator,
        })
    }
}

struct Immutable {
    signing_key: SigningKey,
    public_key: PublicKey,
    account_id: AccountId,
    account_number: AccountNumber,
    fee_token: String,
    gas_and_fee_configuration: GasAndFeeConfiguration,
    chain_id: ChainId,
}
