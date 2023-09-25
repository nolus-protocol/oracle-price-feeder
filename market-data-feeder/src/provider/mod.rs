use std::{error::Error as StdError, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use chain_comms::client::Client as NodeClient;

use crate::config::ProviderExtTrait;

pub(crate) use self::error::Error;

mod error;

#[async_trait]
pub(crate) trait Provider: Sync + Send + 'static {
    async fn get_prices(&self, fault_tolerant: bool) -> Result<Box<[Price]>, Error>;
}

#[async_trait]
pub(crate) trait ProviderSized<Config>: Provider + Sized
where
    Config: ProviderExtTrait,
{
    const ID: &'static str;

    type ConstructError: StdError + Send + 'static;

    async fn from_config(
        id: &str,
        config: &Config,
        oracle_addr: &Arc<str>,
        nolus_client: &Arc<NodeClient>,
    ) -> Result<Self, Self::ConstructError>
    where
        Self: Sized;
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[must_use]
pub(crate) struct Coin {
    amount: u128,
    ticker: String,
}

impl Coin {
    pub const fn new(amount: u128, ticker: String) -> Self {
        Self { amount, ticker }
    }

    pub const fn amount(&self) -> u128 {
        self.amount
    }

    pub fn ticker(&self) -> &str {
        self.ticker.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[must_use]
pub(crate) struct Price {
    amount: Coin,
    amount_quote: Coin,
}

impl Price {
    pub const fn new(
        base_ticker: String,
        base_amount: u128,
        quote_ticker: String,
        quote_amount: u128,
    ) -> Self {
        Self::new_from_coins(
            Coin::new(base_amount, base_ticker),
            Coin::new(quote_amount, quote_ticker),
        )
    }

    pub const fn new_from_coins(amount: Coin, amount_quote: Coin) -> Self {
        Price {
            amount,
            amount_quote,
        }
    }

    pub const fn amount(&self) -> &Coin {
        &self.amount
    }

    pub const fn amount_quote(&self) -> &Coin {
        &self.amount_quote
    }
}
