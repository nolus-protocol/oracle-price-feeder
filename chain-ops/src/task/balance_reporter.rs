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
    idle_duration: Duration,
}

impl BalanceReporter {
    #[inline]
    pub const fn new(
        client: node::QueryBank,
        signer_address: Box<str>,
        denom: Box<str>,
        idle_duration: Duration,
    ) -> Self {
        Self {
            client,
            address: signer_address,
            denom,
            idle_duration,
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

            sleep(self.idle_duration).await;
        }
    }
}

#[test]
fn test_amount_formatting() {
    assert_eq!(BalanceReporter::format_amount("1".into()), "1");

    assert_eq!(BalanceReporter::format_amount("12".into()), "12");

    assert_eq!(BalanceReporter::format_amount("123".into()), "123");

    assert_eq!(BalanceReporter::format_amount("1234".into()), "1 234");

    assert_eq!(BalanceReporter::format_amount("12345".into()), "12 345");

    assert_eq!(BalanceReporter::format_amount("123456".into()), "123 456");

    assert_eq!(
        BalanceReporter::format_amount("1234567".into()),
        "1 234 567"
    );
}
