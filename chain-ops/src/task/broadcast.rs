use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use cosmrs::{
    proto::cosmos::base::abci::v1beta1::TxResponse,
    tendermint::abci::Code as TxCode,
    tx::{Body, Raw, Raw as RawTx},
    Gas,
};
use tokio::{sync::mpsc, time::sleep};

use crate::{channel, node, signer::Signer, supervisor::configuration};

use super::{BuiltIn, Runnable, RunnableState, TxExpiration, TxPackage};

macro_rules! log_simulation {
    ($macro:ident![$source:expr]($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "simulation",
            source = %$source,
            $($body)+
        );
    };
}

macro_rules! log_broadcast {
    ($macro:ident!($($body:tt)+)) => {
        ::tracing::$macro!(
            target: "broadcast",
            $($body)+
        );
    };
}

macro_rules! log_broadcast_with_source {
    ($macro:ident![$source:expr]($($body:tt)+)) => {
        log_broadcast!(
            $macro!(
                source = %$source,
                $($body)+
            )
        );
    };
}

#[must_use]
pub struct Broadcast<Expiration>
where
    Expiration: TxExpiration,
{
    client: node::BroadcastTx,
    signer: Signer,
    transaction_rx: mpsc::UnboundedReceiver<TxPackage<Expiration>>,
    delay_duration: Duration,
    retry_delay_duration: Duration,
    consecutive_errors: u8,
}

impl<Expiration> Broadcast<Expiration>
where
    Expiration: TxExpiration,
{
    #[inline]
    pub const fn new(
        client: node::BroadcastTx,
        signer: Signer,
        transaction_rx: mpsc::UnboundedReceiver<TxPackage<Expiration>>,
        delay_duration: Duration,
        retry_delay_duration: Duration,
    ) -> Self {
        Self {
            client,
            signer,
            transaction_rx,
            delay_duration,
            retry_delay_duration,
            consecutive_errors: 0,
        }
    }

    async fn simulate_and_sign_tx(
        &mut self,
        tx: &Body,
        source: &Arc<str>,
        hard_gas_limit: Gas,
        fallback_gas: Gas,
    ) -> Result<RawTx> {
        let result = self
            .client
            .simulate(
                self.signer
                    .tx(tx, hard_gas_limit)
                    .context("Failed to sign simulation transaction!")?,
            )
            .await;

        match result {
            Ok(gas) => {
                log_simulation!(info![source]("Estimated gas: {gas}"));

                self.signer.tx_with_gas_adjustment(tx, gas, hard_gas_limit)
            },
            Err(error) => {
                log_simulation!(error![source](
                    %fallback_gas,
                    ?error,
                    "Simulation failed. Using fallback gas.",
                ));

                self.signer.tx(tx, fallback_gas)
            },
        }
        .context("Failed to sign transaction intended for broadcasting!")
    }

    fn log_tx_response(source: &str, tx_code: TxCode, response: &TxResponse) {
        match tx_code {
            TxCode::Ok => {
                log_broadcast_with_source!(info![source](
                    hash = %response.txhash,
                    "Transaction broadcast successful.",
                ));
            },
            TxCode::Err(code) => {
                log_broadcast_with_source!(error![source](
                    hash = %response.txhash,
                    log = ?response.raw_log,
                    code = %code,
                    "Transaction broadcast failed!",
                ));
            },
        }
    }

    async fn fetch_sequence_number(&mut self) -> Result<()> {
        log_broadcast!(info!("Fetching sequence number."));

        self.signer.fetch_sequence_number().await.map(|()| {
            log_broadcast!(info!(
                value = self.signer.sequence_number(),
                "Fetched sequence number.",
            ));
        })
    }

    async fn broadcast_tx(
        &mut self,
        TxPackage {
            ref tx_body,
            source,
            hard_gas_limit,
            fallback_gas,
            feedback_sender,
            expiration,
        }: TxPackage<Expiration>,
    ) -> Result<()> {
        const SIGNATURE_VERIFICATION_ERROR_CODE: u32 = 32;

        'broadcast_loop: loop {
            let raw_tx = self
                .simulate_and_sign_tx(
                    tx_body,
                    &source,
                    hard_gas_limit,
                    fallback_gas,
                )
                .await
                .context("Failed to simulate and sign transaction!")?;

            let Some(broadcast_result) = self
                .broadcast_with_expiration(&source, expiration, raw_tx)
                .await
            else {
                break 'broadcast_loop Ok(());
            };

            'process: {
                let response = match broadcast_result {
                    Ok(response) => response,
                    Err(error) => {
                        log_broadcast_with_source!(error![source](
                            ?error,
                            "Broadcasting transaction failed!",
                        ));

                        break 'process;
                    },
                };

                let tx_code: TxCode = response.code.into();

                if tx_code.is_ok()
                    || tx_code.value() == SIGNATURE_VERIFICATION_ERROR_CODE
                {
                    self.signer.increment_sequence_number();
                }

                Self::log_tx_response(source.as_ref(), tx_code, &response);

                if tx_code.is_ok() {
                    self.consecutive_errors = 0;
                } else {
                    self.consecutive_errors = (self.consecutive_errors + 1) % 5;

                    if self.consecutive_errors == 0 {
                        self.fetch_sequence_number()
                            .await
                            .context("Failed to fetch sequence number!")?;
                    }
                }

                if tx_code.value() != SIGNATURE_VERIFICATION_ERROR_CODE {
                    _ = feedback_sender.send(response);

                    break 'broadcast_loop Ok(());
                }
            }

            sleep(self.retry_delay_duration).await;
        }
    }

    async fn broadcast_with_expiration(
        &mut self,
        source: &Arc<str>,
        expiration: Expiration,
        raw_tx: Raw,
    ) -> Option<Result<TxResponse>> {
        Some(
            match expiration.with_expiration(self.client.sync(raw_tx)).await {
                Ok(result) => result,
                Err(error) => {
                    log_broadcast_with_source!(error![source](
                        ?error,
                        "Transaction expired before being committed to the \
                        transactions pool.",
                    ));

                    return None;
                },
            },
        )
    }
}

impl<Expiration> Runnable for Broadcast<Expiration>
where
    Expiration: TxExpiration,
{
    async fn run(mut self, _: RunnableState) -> Result<()> {
        loop {
            let tx_package = self
                .transaction_rx
                .recv()
                .await
                .context("Transaction receiving channel closed!")?;

            self.broadcast_tx(tx_package)
                .await
                .context("Failed to broadcast transaction!")?;

            sleep(self.delay_duration).await;
        }
    }
}

impl<Expiration> BuiltIn for Broadcast<Expiration>
where
    Expiration: TxExpiration,
{
    type ServiceConfiguration = configuration::Service;
}

impl<Expiration> super::Broadcast for Broadcast<Expiration>
where
    Expiration: TxExpiration,
{
    type TxExpiration = Expiration;

    #[inline]
    fn new(
        service_configuration: &Self::ServiceConfiguration,
        transaction_rx: channel::unbounded::Receiver<
            TxPackage<Self::TxExpiration>,
        >,
    ) -> Self {
        Self::new(
            service_configuration.node_client().clone().broadcast_tx(),
            service_configuration.signer().clone(),
            transaction_rx,
            service_configuration.broadcast_delay_duration(),
            service_configuration.broadcast_retry_delay_duration(),
        )
    }
}
