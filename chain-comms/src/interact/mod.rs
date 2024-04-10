use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    num::{NonZeroU128, NonZeroU64},
};

use cosmrs::{proto::cosmos::base::abci::v1beta1::GasInfo, tx::Fee, Coin};
use tracing::error;

use crate::config::Node;

pub mod commit;
pub mod get_tx_response;
pub mod healthcheck;
pub mod query;
pub mod simulate;

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct TxHash(pub String);

impl Display for TxHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(&self.0)
    }
}

#[must_use]
pub fn adjust_gas_limit(
    node_config: &Node,
    gas_limit: NonZeroU64,
    hard_gas_limit: NonZeroU64,
) -> NonZeroU64 {
    NonZeroU128::from(gas_limit)
        .checked_mul(node_config.gas_adjustment_numerator().into())
        .map(|result| {
            result
                .get()
                .checked_div(
                    node_config.gas_adjustment_denominator().get().into(),
                )
                .and_then(NonZeroU128::new)
                .unwrap_or(NonZeroU128::MIN)
        })
        .map_or(gas_limit, |result| {
            NonZeroU64::try_from(result).unwrap_or(hard_gas_limit)
        })
        .min(hard_gas_limit)
}

#[must_use]
pub fn process_simulation_result(
    simulated_tx_result: Result<GasInfo, simulate::error::Error>,
    fallback_gas_limit: NonZeroU64,
) -> NonZeroU64 {
    match simulated_tx_result {
        Ok(gas_info) => {
            NonZeroU64::new(gas_info.gas_used).unwrap_or(NonZeroU64::MIN)
        },
        Err(error) => {
            error!(
                error = %error,
                "Failed to simulate transaction! Falling back to provided gas limit. Fallback gas limit: {gas_limit}.",
                gas_limit = fallback_gas_limit.get(),
            );

            fallback_gas_limit
        },
    }
}

#[must_use]
pub fn calculate_fee(config: &Node, gas_limit: NonZeroU64) -> Fee {
    Fee::from_amount_and_gas(
        Coin {
            denom: config.fee_denom().clone(),
            amount: u128::from(gas_limit.get())
                .saturating_mul(config.gas_price_numerator().get().into())
                .saturating_div(config.gas_price_denominator().get().into())
                .saturating_mul(config.fee_adjustment_numerator().get().into())
                .saturating_div(
                    config.fee_adjustment_denominator().get().into(),
                ),
        },
        gas_limit,
    )
}
