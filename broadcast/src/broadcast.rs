use std::{collections::BTreeMap, time::Duration};

use tokio::{
    spawn,
    time::{sleep, Instant},
};

use chain_comms::{client::Client as NodeClient, interact::commit};

use crate::{
    generators::{CommitError, CommitErrorType, CommitResultSender},
    log, mode,
    preprocess::TxRequest,
    ApiAndConfiguration,
};

pub(crate) struct ProcessingOutput {
    pub(crate) broadcast_timestamp: Instant,
    pub(crate) error: Option<ProcessingError>,
    pub(crate) channel_closed: Option<usize>,
}

pub(crate) enum ProcessingError {
    VerificationFailed,
    SequenceMismatch,
}

#[inline]
#[allow(clippy::future_not_send)]
pub(crate) async fn sleep_and_broadcast_tx<Impl: mode::Impl>(
    api_and_configuration: &mut ApiAndConfiguration,
    between_tx_margin_time: Duration,
    tx_request: TxRequest<Impl>,
    tx_result_senders: &BTreeMap<usize, CommitResultSender>,
    last_signing_timestamp: Instant,
) -> Result<ProcessingOutput, TxRequest<Impl>> {
    sleep_between_txs(between_tx_margin_time, last_signing_timestamp).await;

    broadcast_and_send_back_tx_hash::<Impl>(
        api_and_configuration,
        tx_result_senders,
        tx_request.sender_id,
        tx_request.signed_tx_bytes,
    )
    .await
    .map_err(|signed_tx_bytes| TxRequest {
        signed_tx_bytes,
        ..tx_request
    })
}

#[inline]
async fn sleep_between_txs(
    between_tx_margin_time: Duration,
    last_signing_timestamp: Instant,
) {
    let time_left_since_last_signing: Duration =
        between_tx_margin_time.saturating_sub(last_signing_timestamp.elapsed());

    if !time_left_since_last_signing.is_zero() {
        sleep(time_left_since_last_signing).await;
    }
}

enum SendBackTxHashResult {
    Ok,
    ChannelClosed,
}

#[inline]
#[allow(clippy::future_not_send)]
async fn broadcast_and_send_back_tx_hash<Impl: mode::Impl>(
    &mut ApiAndConfiguration {
        ref node_client,
        ref mut signer,
        tick_time,
        poll_time,
        ..
    }: &mut ApiAndConfiguration,
    tx_result_senders: &BTreeMap<usize, CommitResultSender>,
    sender_id: usize,
    signed_tx_bytes: Vec<u8>,
) -> Result<ProcessingOutput, Vec<u8>> {
    const VERIFICATION_FAILED_CODE: u32 = 4;
    const ACCOUNT_SEQUENCE_MISMATCH_CODE: u32 = 32;

    let tx_response: commit::Response =
        Impl::broadcast_commit(node_client, signer, signed_tx_bytes).await?;

    let processing_error = if tx_response.code.is_err() {
        let code = tx_response.code.value();

        match code {
            VERIFICATION_FAILED_CODE => {
                Some(ProcessingError::VerificationFailed)
            },
            ACCOUNT_SEQUENCE_MISMATCH_CODE => {
                Some(ProcessingError::SequenceMismatch)
            },
            _ => None,
        }
    } else {
        None
    };

    let broadcast_timestamp: Instant = Instant::now();

    log::commit_response(&tx_response);

    let channel_closed: bool = matches!(
        send_back_tx_hash(
            node_client,
            tick_time,
            poll_time,
            tx_result_senders,
            sender_id,
            tx_response,
        ),
        SendBackTxHashResult::ChannelClosed
    );

    Ok(ProcessingOutput {
        broadcast_timestamp,
        error: processing_error,
        channel_closed: channel_closed.then_some(sender_id),
    })
}

#[inline]
fn send_back_tx_hash(
    node_client: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    tx_result_senders: &BTreeMap<usize, CommitResultSender>,
    sender_id: usize,
    tx_response: commit::Response,
) -> SendBackTxHashResult {
    let tx_hash = tx_response.tx_hash.clone();

    let channel_closed = if let Some(sender) = tx_result_senders.get(&sender_id)
    {
        if sender
            .send(if tx_response.code.is_ok() {
                Ok(tx_response.tx_hash)
            } else {
                Err(CommitError {
                    r#type: if tx_response.code.value() == 32 {
                        CommitErrorType::InvalidAccountSequence
                    } else {
                        CommitErrorType::Unknown
                    },
                    tx_response,
                })
            })
            .is_ok()
        {
            return SendBackTxHashResult::Ok;
        }

        SendBackTxHashResult::ChannelClosed
    } else {
        SendBackTxHashResult::Ok
    };

    drop(spawn({
        let node_client = node_client.clone();

        async move {
            crate::poll_delivered_tx(
                &node_client,
                tick_time,
                poll_time,
                tx_hash,
            )
            .await;
        }
    }));

    channel_closed
}
