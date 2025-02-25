use std::{future::Future, sync::Arc, time::Duration};

use anyhow::Result;
use tokio::time::sleep;

use chain_ops::node::QueryBank;
use task::RunnableState;

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

#[derive(Clone)]
#[must_use]
pub struct State {
    pub query_bank: QueryBank,
    pub address: Arc<str>,
    pub denom: Arc<str>,
    pub idle_duration: Duration,
}

impl State {
    pub fn run(
        self,
        _: RunnableState,
    ) -> impl Future<Output = Result<()>> + Sized + use<> {
        let Self {
            mut query_bank,
            address,
            denom,
            idle_duration,
        } = self;

        async move {
            loop {
                let amount = query_bank
                    .balance(address.to_string(), denom.to_string())
                    .await?
                    .to_string();

                log_span!(info_span!("Balance Report") {
                    log!(info!(""));

                    log!(info!("Account address: {address}"));

                    log!(info!(
                        "Amount available: {amount} {denom}",
                        amount = format_amount(amount),
                    ));

                    log!(info!(""));
                });

                sleep(idle_duration).await;
            }
        }
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

#[test]
fn test_amount_formatting() {
    assert_eq!(format_amount("1".into()), "1");

    assert_eq!(format_amount("12".into()), "12");

    assert_eq!(format_amount("123".into()), "123");

    assert_eq!(format_amount("1234".into()), "1 234");

    assert_eq!(format_amount("12345".into()), "12 345");

    assert_eq!(format_amount("123456".into()), "123 456");

    assert_eq!(format_amount("1234567".into()), "1 234 567");
}
