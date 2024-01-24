use std::{collections::BTreeMap, num::NonZeroU64, time::Duration};

use tokio::{
    select, spawn,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::JoinSet,
    time::{Instant, sleep, timeout},
};
use tracing::{error, error_span, info, warn};

use chain_comms::{
    client::Client as NodeClient,
    config::Node as NodeConfig,
    interact::{
        adjust_gas_limit, calculate_fee, commit, get_tx_response, process_simulation_result,
        simulate,
    },
    reexport::cosmrs::{
        Any as ProtobufAny,
        rpc::error::{Error as RpcError, ErrorDetail as RpcErrorDetail},
        tendermint::{abci::response::DeliverTx, Hash as TxHash},
        tx::Body as TxBody,
    },
    signer::Signer,
};

use crate::config::Config;

pub mod config;
mod log;

pub type TxRequestSender = UnboundedSender<TxRequest>;

pub struct TxRequest {
    sender_id: usize,
    messages: Vec<ProtobufAny>,
    hard_gas_limit: NonZeroU64,
}

impl TxRequest {
    pub const fn new(
        sender_id: usize,
        messages: Vec<ProtobufAny>,
        hard_gas_limit: NonZeroU64,
    ) -> Self {
        Self {
            sender_id,
            messages,
            hard_gas_limit,
        }
    }
}

pub struct SpawnGeneratorsResult {
    tx_generators_set: JoinSet<()>,
    tx_result_senders: BTreeMap<usize, CommitResultSender>,
}

impl SpawnGeneratorsResult {
    pub const fn new(
        tx_generators_set: JoinSet<()>,
        tx_result_senders: BTreeMap<usize, CommitResultSender>,
    ) -> Self {
        Self {
            tx_generators_set,
            tx_result_senders,
        }
    }
}

pub async fn broadcast<F, E>(
    signer: Signer,
    config: Config,
    node_client: NodeClient,
    node_config: NodeConfig,
    spawn_generators: F,
) -> Result<(), E>
    where
        F: FnOnce(TxRequestSender) -> Result<SpawnGeneratorsResult, E>,
{
    let (tx_sender, mut tx_receiver) = unbounded_channel();

    let SpawnGeneratorsResult {
        mut tx_generators_set,
        mut tx_result_senders,
    } = spawn_generators(tx_sender)?;

    let mut last_signing_timestamp = Instant::now();

    let mut fallback_gas_limit: u64 = 0;

    let mut process_transaction_env = ProcessTransactionRequestContext {
        node_client,
        node_config,
        signer,
        tick_time: config.tick_time,
        poll_time: config.poll_time,
    };

    loop {
        select! {
            Some(_) = tx_generators_set.join_next(), if !tx_generators_set.is_empty() => {},
            Some(TxRequest { sender_id, messages, hard_gas_limit }) = tx_receiver.recv() => {
                if last_signing_timestamp.elapsed() < config.between_tx_margin_time {
                    sleep(config.between_tx_margin_time).await;
                }

                if let Some(ProcessTransactionRequestResult {
                    broadcast_timestamp,
                    new_fallback_gas_limit,
                    channel_closed,
                }) = process_transaction_request(
                    &mut process_transaction_env,
                    fallback_gas_limit,
                    &tx_result_senders,
                    sender_id,
                    messages,
                    hard_gas_limit,
                )
                .await
                {
                    last_signing_timestamp = broadcast_timestamp;

                    fallback_gas_limit = new_fallback_gas_limit;

                    if channel_closed {
                        let _ = tx_result_senders.remove(&sender_id);
                    }
                }
            }
            else => {
                info!("All generator threads stopped. Exiting.");

                break;
            }
        }
    }

    tx_generators_set.shutdown().await;

    Ok(())
}

struct ProcessTransactionRequestContext {
    node_client: NodeClient,
    node_config: NodeConfig,
    signer: Signer,
    tick_time: Duration,
    poll_time: Duration,
}

struct ProcessTransactionRequestResult {
    broadcast_timestamp: Instant,
    new_fallback_gas_limit: u64,
    channel_closed: bool,
}

async fn process_transaction_request(
    &mut ProcessTransactionRequestContext {
        ref node_client,
        ref node_config,
        ref mut signer,
        tick_time,
        poll_time,
    }: &mut ProcessTransactionRequestContext,
    mut fallback_gas_limit: u64,
    tx_result_senders: &BTreeMap<usize, CommitResultSender>,
    sender_id: usize,
    messages: Vec<ProtobufAny>,
    hard_gas_limit: NonZeroU64,
) -> Option<ProcessTransactionRequestResult> {
    let tx_body: TxBody = TxBody::new(messages, String::new(), 0_u32);

    let Some(signed_tx_bytes) =
        sign_and_serialize_tx(signer, node_config, hard_gas_limit.get(), tx_body.clone())
        else {
            return None;
        };

    let simulation_result =
        simulate::with_signed_body(node_client, signed_tx_bytes, hard_gas_limit.get()).await;

    let estimated_gas_limit = process_simulation_result(simulation_result, fallback_gas_limit);

    let gas_limit = adjust_gas_limit(node_config, estimated_gas_limit, hard_gas_limit.get());

    fallback_gas_limit = gas_limit.max(fallback_gas_limit);

    let Some(signed_tx_bytes) = sign_and_serialize_tx(signer, node_config, gas_limit, tx_body)
        else {
            return None;
        };

    let tx_response = broadcast_commit(node_client, signer, signed_tx_bytes).await;

    let broadcast_timestamp = Instant::now();

    log::commit_response(&tx_response);

    let channel_closed = matches!(
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

    Some(ProcessTransactionRequestResult {
        broadcast_timestamp,
        new_fallback_gas_limit: fallback_gas_limit,
        channel_closed,
    })
}

enum SendBackTxHashResult {
    Ok,
    ChannelClosed,
}

fn send_back_tx_hash(
    node_client: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    tx_result_senders: &BTreeMap<usize, CommitResultSender>,
    sender_id: usize,
    tx_response: commit::Response,
) -> SendBackTxHashResult {
    let hash = tx_response.hash;

    let channel_closed = if let Some(sender) = tx_result_senders.get(&sender_id) {
        if sender
            .send(if tx_response.code.is_ok() {
                Ok(tx_response.hash)
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
            poll_delivered_tx(&node_client, tick_time, poll_time, hash).await;
        }
    }));

    channel_closed
}

async fn broadcast_commit(
    node_client: &NodeClient,
    signer: &mut Signer,
    signed_tx_bytes: Vec<u8>,
) -> commit::Response {
    loop {
        match commit::with_signed_body(node_client, signed_tx_bytes.clone(), signer).await {
            Ok(tx_response) => {
                break tx_response;
            }
            Err(error) => {
                error_span!("Broadcast").in_scope(|| {
                    if let commit::error::CommitTx::Broadcast(
                        RpcError(
                            RpcErrorDetail::Timeout(..),
                            ..,
                        ),
                    ) = error {
                        warn!(error = ?error, "Failed to broadcast transaction due to a timeout! Cause: {}", error);
                    } else {
                        error!(error = ?error, "Failed to broadcast transaction due to an error! Cause: {}", error);
                    }

                    info!("Retrying to broadcast.");
                });
            }
        }
    }
}

fn sign_and_serialize_tx(
    signer: &mut Signer,
    node_config: &NodeConfig,
    gas_limit: u64,
    tx_body: TxBody,
) -> Option<Vec<u8>> {
    match signer.sign(tx_body, calculate_fee(node_config, gas_limit)) {
        Ok(signed_tx) => match signed_tx.to_bytes() {
            Ok(tx_bytes) => {
                return Some(tx_bytes);
            }
            Err(error) => {
                error!(error = ?error, "Serializing signed transaction failed! Cause: {}", error);
            }
        },
        Err(error) => {
            error!(error = ?error, "Signing transaction failed! Cause: {}", error);
        }
    }

    None
}

pub enum CommitErrorType {
    InvalidAccountSequence,
    Unknown,
}

pub struct CommitError {
    pub r#type: CommitErrorType,
    pub tx_response: commit::Response,
}

pub type CommitResult = Result<TxHash, CommitError>;

pub type CommitResultSender = UnboundedSender<CommitResult>;

pub type CommitResultReceiver = UnboundedReceiver<CommitResult>;

pub async fn poll_delivered_tx(
    node_client: &NodeClient,
    tick_time: Duration,
    poll_time: Duration,
    hash: TxHash,
) -> Option<DeliverTx> {
    timeout(tick_time, async {
        loop {
            sleep(poll_time).await;

            match get_tx_response(node_client, hash).await {
                Ok(tx) => {
                    break tx;
                }
                Err(error) => {
                    error!(
                        hash = %hash,
                        error = ?error,
                        "Polling delivered transaction failed!",
                    );
                }
            }
        }
    })
        .await
        .ok()
}
