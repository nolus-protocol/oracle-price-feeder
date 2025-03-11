use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use cosmrs::{
    proto::cosmos::base::abci::v1beta1::TxResponse,
    tendermint::abci::Code as TxCode,
    tx::{Body as TxBody, Raw as RawTx},
};
use tokio::{
    sync::{Mutex, OwnedMutexGuard},
    time::sleep,
};

use chain_ops::{
    node::BroadcastTx,
    signer::{Gas, Signer},
};
use channel::unbounded;
use environment::ReadFromVar;
use task::{Run, RunnableState};
use tx::{TxExpiration, TxPackage};

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

pub struct Environment {
    delay_duration: Duration,
    retry_delay_duration: Duration,
}

impl Environment {
    pub fn read_from_env() -> Result<Self> {
        Ok(Self {
            delay_duration: ReadFromVar::read_from_var(
                "BROADCAST_DELAY_DURATION_SECONDS",
            )
            .map(Duration::from_secs)?,
            retry_delay_duration: ReadFromVar::read_from_var(
                "BROADCAST_RETRY_DELAY_DURATION_MILLISECONDS",
            )
            .map(Duration::from_secs)?,
        })
    }
}

#[derive(Clone)]
#[must_use]
pub struct State<TxExpiration>
where
    TxExpiration: Send,
{
    broadcast_tx: BroadcastTx,
    signer: Arc<Mutex<Signer>>,
    transaction_rx: Arc<Mutex<unbounded::Receiver<TxPackage<TxExpiration>>>>,
    delay_duration: Duration,
    retry_delay_duration: Duration,
}

impl<TxExpiration> State<TxExpiration>
where
    TxExpiration: Send,
{
    #[inline]
    pub const fn new(
        Environment {
            delay_duration,
            retry_delay_duration,
        }: Environment,
        broadcast_tx: BroadcastTx,
        signer: Arc<Mutex<Signer>>,
        transaction_rx: Arc<
            Mutex<unbounded::Receiver<TxPackage<TxExpiration>>>,
        >,
    ) -> Self {
        Self {
            broadcast_tx,
            signer,
            transaction_rx,
            delay_duration,
            retry_delay_duration,
        }
    }
}

impl<TxExpiration> Run for State<TxExpiration>
where
    TxExpiration: self::TxExpiration,
{
    async fn run(self, _: RunnableState) -> Result<()> {
        let Self {
            broadcast_tx,
            signer,
            transaction_rx,
            delay_duration,
            retry_delay_duration,
        } = self;

        let mut transaction_rx = transaction_rx.lock_owned().await;

        let mut broadcast = Broadcast::new(
            broadcast_tx,
            signer.lock_owned().await,
            retry_delay_duration,
        );

        loop {
            let tx_package = transaction_rx
                .recv()
                .await
                .context("Transaction receiving channel closed!")?;

            broadcast
                .broadcast_tx(tx_package)
                .await
                .context("Failed to broadcast transaction!")?;

            sleep(delay_duration).await;
        }
    }
}

#[must_use]
pub struct Broadcast {
    client: BroadcastTx,
    signer: OwnedMutexGuard<Signer>,
    retry_delay_duration: Duration,
    consecutive_errors: u8,
}

impl Broadcast {
    #[inline]
    pub const fn new(
        client: BroadcastTx,
        signer: OwnedMutexGuard<Signer>,
        retry_delay_duration: Duration,
    ) -> Self {
        Self {
            client,
            signer,
            retry_delay_duration,
            consecutive_errors: 0,
        }
    }

    async fn simulate_and_sign_tx(
        &mut self,
        tx: &TxBody,
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
}

impl Broadcast {
    async fn broadcast_tx<Expiration>(
        &mut self,
        TxPackage {
            ref tx_body,
            source,
            hard_gas_limit,
            fallback_gas,
            feedback_sender,
            expiration,
        }: TxPackage<Expiration>,
    ) -> Result<()>
    where
        Expiration: TxExpiration,
    {
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

    async fn broadcast_with_expiration<Expiration>(
        &mut self,
        source: &Arc<str>,
        expiration: Expiration,
        raw_tx: RawTx,
    ) -> Option<Result<TxResponse>>
    where
        Expiration: TxExpiration,
    {
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
