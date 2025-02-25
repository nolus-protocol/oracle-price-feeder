use std::{convert::Infallible, error::Error, future::Future, sync::Arc};

use anyhow::Result;
use cosmrs::{
    proto::cosmos::base::abci::v1beta1::TxResponse, tx::Body as TxBody,
};
use tokio::{
    sync::oneshot,
    time::{error::Elapsed, timeout_at, Instant},
};

use chain_ops::signer::Gas;

pub struct TxPackage<Expiration> {
    pub tx_body: TxBody,
    pub source: Arc<str>,
    pub hard_gas_limit: Gas,
    pub fallback_gas: Gas,
    pub feedback_sender: oneshot::Sender<TxResponse>,
    pub expiration: Expiration,
}

pub trait TxExpiration: Copy + Send + Sized + 'static {
    type Expired: Error + 'static;

    fn with_expiration<F>(
        self,
        future: F,
    ) -> impl Future<Output = Result<F::Output, Self::Expired>> + Send
    where
        F: Future + Send;
}

#[derive(Clone, Copy)]
#[must_use]
pub struct NoExpiration;

impl TxExpiration for NoExpiration {
    type Expired = Infallible;

    #[inline]
    async fn with_expiration<F>(
        self,
        future: F,
    ) -> Result<F::Output, Self::Expired>
    where
        F: Future + Send,
    {
        Ok(future.await)
    }
}

#[derive(Clone, Copy)]
#[must_use]
pub struct TimeBasedExpiration {
    expires_at: Instant,
}

impl TimeBasedExpiration {
    pub const fn new(expires_at: Instant) -> Self {
        Self { expires_at }
    }
}

impl TxExpiration for TimeBasedExpiration {
    type Expired = Elapsed;

    #[inline]
    fn with_expiration<F>(
        self,
        future: F,
    ) -> impl Future<Output = Result<F::Output, Self::Expired>> + Send
    where
        F: Future + Send,
    {
        timeout_at(self.expires_at, future)
    }
}
