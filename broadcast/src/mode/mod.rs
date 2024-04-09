use tokio::time::Instant;
use tracing::{error, error_span, info, warn};

use chain_comms::{
    client::Client as NodeClient,
    interact::{
        commit,
        healthcheck::{
            error::Error as HealthcheckError, Healthcheck,
            WaitUntilHealthyStatusType,
        },
    },
    reexport::tonic::Code as TonicStatusCode,
    signer::Signer,
};

use crate::cache;

pub(crate) use self::sealed::{FilterResult, Impl, PurgeResult};

mod sealed;

pub struct Blocking;

impl Impl for Blocking {
    type Expiration = ();

    async fn healthcheck(
        healthcheck: &mut Healthcheck,
    ) -> Result<(), HealthcheckError> {
        let mut counter: u8 = 0;

        let mut last_status = WaitUntilHealthyStatusType::Syncing;

        healthcheck
            .wait_until_healthy(
                move |status_type| {
                    if status_type != last_status {
                        counter = 0;

                        last_status = status_type;
                    }

                    if counter == 0 {
                        match status_type {
                            WaitUntilHealthyStatusType::Syncing => {
                                warn!(
                                    "Connected node responded with syncing \
                                    status."
                                );
                            },
                            WaitUntilHealthyStatusType::BlockNotIncremented => {
                                warn!(
                                    "Connected node didn't respond with an \
                                    incremented block height."
                                );
                            },
                        }
                    }

                    counter = (counter + 1) % 10;
                },
                move || info!("Connected node is healthy again."),
            )
            .await
    }

    #[inline]
    fn purge_cache(cache: &mut cache::TxRequests<Self>) -> PurgeResult {
        if cache.values_mut().any(|slot| slot.get_mut().is_some()) {
            PurgeResult::NotExhausted
        } else {
            PurgeResult::Exhausted
        }
    }

    #[inline]
    fn filter((): &Self::Expiration) -> FilterResult {
        FilterResult::NotExpired
    }

    #[allow(clippy::future_not_send)]
    async fn broadcast_commit(
        node_client: &NodeClient,
        signer: &mut Signer,
        signed_tx_bytes: Vec<u8>,
    ) -> Result<commit::Response, Vec<u8>> {
        loop {
            if let Some(tx_response) =
                try_commit(node_client, signer, signed_tx_bytes.clone(), || {
                    info!("Retrying to broadcast.");
                })
                .await
            {
                break Ok(tx_response);
            }
        }
    }
}

pub struct NonBlocking;

impl Impl for NonBlocking {
    type Expiration = Instant;

    async fn healthcheck(_: &mut Healthcheck) -> Result<(), HealthcheckError> {
        Ok(())
    }

    #[inline]
    fn purge_cache(cache: &mut cache::TxRequests<Self>) -> PurgeResult {
        let mut exhausted: bool = true;

        let now: Instant = Instant::now();

        cache.values_mut().for_each({
            |slot| {
                let expired: bool =
                    slot.get_mut().as_ref().map_or(false, |tx_request| {
                        let expired: bool = tx_request.expiration <= now;

                        exhausted &= expired;

                        expired
                    });

                if expired {
                    _ = slot.take();
                }
            }
        });

        if exhausted {
            PurgeResult::Exhausted
        } else {
            PurgeResult::NotExhausted
        }
    }

    #[inline]
    fn filter(expiration: &Self::Expiration) -> FilterResult {
        if *expiration > Instant::now() {
            FilterResult::NotExpired
        } else {
            FilterResult::Expired
        }
    }

    #[inline]
    #[allow(clippy::future_not_send)]
    async fn broadcast_commit(
        node_client: &NodeClient,
        signer: &mut Signer,
        signed_tx_bytes: Vec<u8>,
    ) -> Result<commit::Response, Vec<u8>> {
        try_commit(node_client, signer, signed_tx_bytes.clone(), || {})
            .await
            .ok_or(signed_tx_bytes)
    }
}

#[allow(clippy::future_not_send)]
async fn try_commit<F>(
    node_client: &NodeClient,
    signer: &mut Signer,
    signed_tx_bytes: Vec<u8>,
    on_error: F,
) -> Option<commit::Response>
where
    F: FnOnce() + Send,
{
    let commit_result: Result<commit::Response, commit::error::CommitTx> =
        commit::with_signed_body(node_client, signed_tx_bytes, signer).await;

    match commit_result {
        Ok(tx_response) => Some(tx_response),
        Err(error) => {
            error_span!("Broadcast").in_scope(|| {
                'log_error: {
                    if let commit::error::CommitTx::Broadcast(error) = &error {
                        if matches!(error.code(), TonicStatusCode::DeadlineExceeded) {
                            warn!(error = ?error, "Failed to broadcast transaction due to a timeout! Cause: {}", error);

                            break 'log_error;
                        }
                    }

                    error!(error = ?error, "Failed to broadcast transaction due to an error! Cause: {}", error);
                }

                on_error();
            });

            None
        },
    }
}
