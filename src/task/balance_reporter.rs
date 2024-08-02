use std::time::Duration;

use anyhow::Result;
use tokio::time::sleep;

use crate::node;

use super::Runnable;

macro_rules! log {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "balance-reporter",
            $($body)+
        );
    };
}

macro_rules! log_span {
    ($macro:ident!($($body:tt)+) $block:block) => {
        ::tracing::$macro!($($body)+).in_scope(|| $block);
    };
}

#[must_use]
pub struct BalanceReporter {
    client: node::QueryBank,
    address: Box<str>,
    denom: Box<str>,
}

impl BalanceReporter {
    #[inline]
    pub const fn new(
        client: node::QueryBank,
        signer_address: Box<str>,
        denom: Box<str>,
    ) -> Self {
        Self {
            client,
            address: signer_address,
            denom,
        }
    }

    fn format_amount(mut amount: String) -> String {
        if amount.len() > 3 {
            let offset = amount.len() % 3;

            (0..amount.len() / 3)
                .rev()
                .map(|triplet| triplet * 3)
                .map(|index| index + offset)
                .filter(|&index| index != 0)
                .for_each(|index| amount.insert(index, ' '));
        }

        amount
    }
}

impl Runnable for BalanceReporter {
    async fn run(mut self) -> Result<()> {
        const IDLE_DURATION: Duration = Duration::from_secs(30);

        loop {
            let amount = self
                .client
                .balance(self.address.to_string(), self.denom.to_string())
                .await?
                .to_string();

            log_span!(info_span!("Balance Report") {
                log!(info!(""));

                log!(info!("Amount available: {} {}", Self::format_amount(amount), self.denom));

                log!(info!(""));
            });

            sleep(IDLE_DURATION).await;
        }
    }
}
