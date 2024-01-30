use std::{collections::BTreeMap, num::NonZeroU32, num::NonZeroU64, sync::Arc, time::Duration};

use tokio::{sync::mpsc::unbounded_channel, task::JoinSet, time::sleep};
use tracing::{error, info, warn};

use broadcast::{
    generators::{
        CommitErrorType, CommitResultReceiver, CommitResultSender, SpawnResult, TxRequest,
        TxRequestSender,
    },
    TimeInsensitive,
};
use chain_comms::{
    client::Client as NodeClient,
    interact::query,
    reexport::cosmrs::{
        proto::cosmwasm::wasm::v1::MsgExecuteContract,
        tendermint::{abci::response::DeliverTx, Hash as TxHash},
        tx::MessageExt as _,
        Any as ProtobufAny,
    },
};

use crate::{
    config::Contract,
    error::DispatchAlarms as DispatchAlarmsError,
    log,
    messages::{ExecuteMsg, QueryMsg, StatusResponse},
};

pub fn spawn(
    node_client: &NodeClient,
    signer_address: &str,
    tx_sender: &TxRequestSender<TimeInsensitive>,
    market_price_oracle_contracts: Vec<Contract>,
    time_alarms_contracts: Vec<Contract>,
    tick_time: Duration,
    poll_time: Duration,
) -> Result<SpawnResult, DispatchAlarmsError> {
    let mut tx_generators_set = JoinSet::new();

    let mut tx_result_senders = BTreeMap::new();

    market_price_oracle_contracts
        .into_iter()
        .map(|contract| (contract, "oracle"))
        .chain(
            time_alarms_contracts
                .into_iter()
                .map(|contract| (contract, "time_alarms")),
        )
        .enumerate()
        .try_for_each(|(monotonic_id, (contract, contract_type))| {
            spawn_single(
                signer_address.to_owned(),
                node_client,
                &mut tx_generators_set,
                &mut tx_result_senders,
                SpawnTxGeneratorContext {
                    tx_sender: tx_sender.clone(),
                    monotonic_id,
                    contract,
                    contract_type,
                },
                tick_time,
                poll_time,
            )
        })
        .map(|()| SpawnResult::new(tx_generators_set, tx_result_senders))
}

struct SpawnTxGeneratorContext {
    tx_sender: TxRequestSender<TimeInsensitive>,
    monotonic_id: usize,
    contract: Contract,
    contract_type: &'static str,
}

fn spawn_single(
    signer_address: String,
    node_client: &NodeClient,
    tx_generators_set: &mut JoinSet<()>,
    tx_result_senders: &mut BTreeMap<usize, CommitResultSender>,
    SpawnTxGeneratorContext {
        tx_sender,
        monotonic_id,
        contract,
        contract_type,
    }: SpawnTxGeneratorContext,
    tick_time: Duration,
    poll_time: Duration,
) -> Result<(), DispatchAlarmsError> {
    let messages: Box<[ProtobufAny]> = {
        let mut message: Vec<ProtobufAny> = vec![MsgExecuteContract {
            sender: signer_address,
            contract: contract.address.clone(),
            msg: serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
                max_count: contract.max_alarms_group.get(),
            })?,
            funds: Vec::new(),
        }
        .to_any()?];

        message.shrink_to_fit();

        message.into()
    };

    let contract_address: Arc<str> = contract.address.into();

    let hard_gas_limit = contract
        .gas_limit_per_alarm
        .saturating_mul(contract.max_alarms_group.into());

    let (tx_result_sender, tx_result_receiver): (CommitResultSender, CommitResultReceiver) =
        unbounded_channel();

    tx_result_senders.insert(monotonic_id, tx_result_sender);

    tx_generators_set.spawn(task(
        node_client.clone(),
        tx_sender,
        tx_result_receiver,
        TaskContext {
            monotonic_id,
            contract_address,
            max_alarms_count: contract.max_alarms_group,
            messages,
            contract_type,
            hard_gas_limit,
        },
        tick_time,
        poll_time,
    ));

    Ok(())
}

struct TaskContext {
    monotonic_id: usize,
    contract_address: Arc<str>,
    max_alarms_count: NonZeroU32,
    messages: Box<[ProtobufAny]>,
    contract_type: &'static str,
    hard_gas_limit: NonZeroU64,
}

async fn task(
    node_client: NodeClient,
    tx_sender: TxRequestSender<TimeInsensitive>,
    result_receiver: CommitResultReceiver,
    context: TaskContext,
    tick_time: Duration,
    poll_time: Duration,
) {
    if let Err(FatalError {
        contract_type,
        contract_address,
        tx_hash,
        response,
    }) = task_inner(
        node_client,
        tx_sender,
        result_receiver,
        context,
        tick_time,
        poll_time,
    )
    .await
    {
        loop {
            error!(
                contract_type = contract_type,
                address = contract_address.as_ref(),
                code = response.code.value(),
                log = response.log,
                data = ?response.data,
                hash = %tx_hash,
                "Task encountered expected error!"
            );

            sleep(tick_time).await;
        }
    }
}

struct FatalError {
    contract_type: &'static str,
    contract_address: Arc<str>,
    tx_hash: TxHash,
    response: DeliverTx,
}

async fn task_inner(
    node_client: NodeClient,
    tx_sender: TxRequestSender<TimeInsensitive>,
    mut result_receiver: CommitResultReceiver,
    context: TaskContext,
    tick_time: Duration,
    poll_time: Duration,
) -> Result<(), FatalError> {
    let mut fallback_gas_limit: NonZeroU64 = context.hard_gas_limit;

    'runner_loop: loop {
        let should_send: bool = node_client
            .with_grpc(|rpc| {
                query::wasm(
                    rpc,
                    context.contract_address.to_string(),
                    QueryMsg::ALARMS_STATUS,
                )
            })
            .await
            .map_or(true, |response: StatusResponse| {
                response.remaining_for_dispatch()
            });

        if should_send {
            'generator_loop: loop {
                if let Err(ChannelClosedError) = send_tx(&tx_sender, &context, fallback_gas_limit) {
                    break 'runner_loop Ok(());
                }

                let tx_hash: TxHash = match receive_back_tx_hash(
                    &mut result_receiver,
                    &context.contract_address,
                    context.contract_type,
                )
                .await
                {
                    Ok(Some(hash)) => hash,
                    Ok(None) => {
                        continue 'generator_loop;
                    }
                    Err(ChannelClosedError) => {
                        break 'runner_loop Ok(());
                    }
                };

                let Some(response): Option<DeliverTx> =
                    broadcast::poll_delivered_tx(&node_client, tick_time, poll_time, tx_hash).await
                else {
                    warn!("Transaction not found or couldn't be reported back in the given specified period.");

                    continue 'generator_loop;
                };

                match handle_response(&context, &mut fallback_gas_limit, tx_hash, response) {
                    HandleResponseResult::ContinueTxLooping => {
                        continue 'generator_loop;
                    }
                    HandleResponseResult::BreakTxLoop => {
                        break 'generator_loop;
                    }
                    HandleResponseResult::Fatal { tx_hash, response } => {
                        break 'runner_loop Err(FatalError {
                            contract_type: context.contract_type,
                            contract_address: context.contract_address,
                            tx_hash,
                            response,
                        });
                    }
                }
            }
        }

        sleep(tick_time).await;
    }
}

enum HandleResponseResult {
    ContinueTxLooping,
    BreakTxLoop,
    Fatal {
        tx_hash: TxHash,
        response: DeliverTx,
    },
}

fn handle_response(
    context: &TaskContext,
    fallback_gas_limit: &mut NonZeroU64,
    tx_hash: TxHash,
    response: DeliverTx,
) -> HandleResponseResult {
    let maybe_dispatched_count: Option<u32> = log::tx_response(
        context.contract_type,
        &context.contract_address,
        &tx_hash,
        &response,
    )
    .map(|dispatch_response| dispatch_response.dispatched_alarms());

    let dispatched_count: u32 = match extract_dispatched_count(
        fallback_gas_limit,
        tx_hash,
        response,
        maybe_dispatched_count,
    ) {
        Ok(dispatched_count) => dispatched_count,
        Err(ExtractDispatchedCountError::OutOfGas) => {
            return HandleResponseResult::ContinueTxLooping;
        }
        Err(ExtractDispatchedCountError::Fatal { tx_hash, response }) => {
            return HandleResponseResult::Fatal { tx_hash, response };
        }
    };

    if dispatched_count < context.max_alarms_count.get() {
        info!("No alarms should be left for the time being.");

        HandleResponseResult::BreakTxLoop
    } else {
        HandleResponseResult::ContinueTxLooping
    }
}

struct ChannelClosedError;

fn send_tx(
    tx_sender: &TxRequestSender<TimeInsensitive>,
    context: &TaskContext,
    fallback_gas_limit: NonZeroU64,
) -> Result<(), ChannelClosedError> {
    let channel_closed: bool = tx_sender
        .send(TxRequest::<TimeInsensitive>::new(
            context.monotonic_id,
            context.messages.to_vec(),
            fallback_gas_limit,
            context.hard_gas_limit,
        ))
        .is_err();

    if channel_closed {
        warn!(
            contract_type = context.contract_type,
            contract_address = context.contract_address.as_ref(),
            "Channel closed. Exiting task.",
        );

        Err(ChannelClosedError)
    } else {
        Ok(())
    }
}

enum ExtractDispatchedCountError {
    OutOfGas,
    Fatal {
        tx_hash: TxHash,
        response: DeliverTx,
    },
}

fn extract_dispatched_count(
    fallback_gas_limit: &mut NonZeroU64,
    tx_hash: TxHash,
    response: DeliverTx,
    maybe_dispatched_count: Option<u32>,
) -> Result<u32, ExtractDispatchedCountError> {
    if response.code.is_err() {
        if response.code.value() == 11 {
            warn!("Transaction failed. Probable error is out of gas. Retrying transaction.");

            if let Some(gas_used) = NonZeroU64::new(response.gas_used.unsigned_abs()) {
                *fallback_gas_limit = gas_used.max(*fallback_gas_limit);
            }

            return Err(ExtractDispatchedCountError::OutOfGas);
        }
    } else {
        {
            let (mut n, overflow) = fallback_gas_limit
                .get()
                .overflowing_add(response.gas_used.unsigned_abs());

            n >>= 1;

            if overflow {
                n |= 1 << (u64::BITS - 1);
            }

            if let Some(n) = NonZeroU64::new(n) {
                *fallback_gas_limit = n;
            }
        }

        if let Some(dispatched_count) = maybe_dispatched_count {
            return Ok(dispatched_count);
        }
    }

    Err(ExtractDispatchedCountError::Fatal { tx_hash, response })
}

async fn receive_back_tx_hash(
    result_receiver: &mut CommitResultReceiver,
    contract_address: &Arc<str>,
    contract_type: &str,
) -> Result<Option<TxHash>, ChannelClosedError> {
    if let Some(result) = result_receiver.recv().await {
        match result {
            Ok(hash) => return Ok(Some(hash)),
            Err(error) => {
                error!(
                    contract_type = contract_type,
                    address = contract_address.as_ref(),
                    code = error.tx_response.code.value(),
                    log = error.tx_response.log,
                    data = ?error.tx_response.data,
                    "Failed to commit transaction! Error type: {}",
                    match error.r#type {
                        CommitErrorType::InvalidAccountSequence => "Invalid account sequence",
                        CommitErrorType::Unknown => "Unknown",
                    },
                );
            }
        }
    } else {
        info!(
            contract_type = contract_type,
            address = contract_address.as_ref(),
            "Transaction result communication channel closed. Exiting task.",
        );

        return Err(ChannelClosedError);
    }

    Ok(None)
}
