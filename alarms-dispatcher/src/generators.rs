use std::{
    collections::BTreeMap,
    convert::Infallible,
    num::{NonZeroU32, NonZeroU64},
    sync::Arc,
    time::Duration,
};

use tokio::{sync::mpsc::unbounded_channel, task::JoinSet, time::sleep};
use tracing::{error, info, warn};

use broadcast::{
    generators::{
        CommitError, CommitErrorType, CommitResultReceiver, CommitResultSender,
        SpawnResult, TxRequest, TxRequestSender,
    },
    mode::Blocking,
};
use chain_comms::{
    client::Client as NodeClient,
    interact::{get_tx_response::Response as TxResponse, query, TxHash},
    reexport::cosmrs::{
        proto::cosmwasm::wasm::v1::MsgExecuteContract, Any as ProtobufAny,
    },
};

use crate::{
    config::AlarmsConfig,
    error::DispatchAlarms as DispatchAlarmsError,
    log,
    messages::{ExecuteMsg, QueryMsg, StatusResponse},
};

pub(crate) enum Contract {
    TimeAlarms(Box<str>),
    Oracle(Box<str>),
}

pub(crate) struct TasksConfig {
    pub time_alarms_config: AlarmsConfig,
    pub oracle_alarms_config: AlarmsConfig,
    pub tick_time: Duration,
    pub poll_time: Duration,
}

pub(crate) fn spawn<I>(
    node_client: &NodeClient,
    signer_address: &str,
    tx_sender: &TxRequestSender<Blocking>,
    tasks_config: &TasksConfig,
    contracts: I,
) -> Result<SpawnResult, DispatchAlarmsError>
where
    I: Iterator<Item = Contract>,
{
    let mut tx_generators_set: JoinSet<Infallible> = JoinSet::new();

    let mut tx_result_senders = BTreeMap::new();

    contracts
        .map(|contract| match contract {
            Contract::TimeAlarms(address) => {
                (address, "time_alarms", &tasks_config.time_alarms_config)
            },
            Contract::Oracle(address) => {
                (address, "oracle", &tasks_config.oracle_alarms_config)
            },
        })
        .enumerate()
        .try_for_each(
            |(monotonic_id, (contract, contract_type, &alarms_config))| {
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
                        alarms_config,
                    },
                    tasks_config.tick_time,
                    tasks_config.poll_time,
                )
            },
        )
        .map(|()| SpawnResult::new(tx_generators_set, tx_result_senders))
}

struct SpawnTxGeneratorContext {
    tx_sender: TxRequestSender<Blocking>,
    monotonic_id: usize,
    contract: Box<str>,
    contract_type: &'static str,
    alarms_config: AlarmsConfig,
}

fn spawn_single(
    signer_address: String,
    node_client: &NodeClient,
    tx_generators_set: &mut JoinSet<Infallible>,
    tx_result_senders: &mut BTreeMap<usize, CommitResultSender>,
    SpawnTxGeneratorContext {
        tx_sender,
        monotonic_id,
        contract,
        contract_type,
        alarms_config,
    }: SpawnTxGeneratorContext,
    tick_time: Duration,
    poll_time: Duration,
) -> Result<(), DispatchAlarmsError> {
    let messages: Box<[ProtobufAny]> = {
        let mut message: Vec<ProtobufAny> =
            vec![ProtobufAny::from_msg(&MsgExecuteContract {
                sender: signer_address,
                contract: contract.clone().into_string(),
                msg: serde_json_wasm::to_vec(&ExecuteMsg::DispatchAlarms {
                    max_count: alarms_config.max_alarms_group.get(),
                })?,
                funds: Vec::new(),
            })?];

        message.shrink_to_fit();

        message.into()
    };

    let contract_address: Arc<str> = contract.into();

    let hard_gas_limit = alarms_config
        .gas_limit_per_alarm
        .saturating_mul(alarms_config.max_alarms_group.into());

    let (tx_result_sender, tx_result_receiver): (
        CommitResultSender,
        CommitResultReceiver,
    ) = unbounded_channel();

    tx_result_senders.insert(monotonic_id, tx_result_sender);

    tx_generators_set.spawn(task(
        node_client.clone(),
        tx_sender,
        tx_result_receiver,
        TaskContext {
            monotonic_id,
            contract_address,
            max_alarms_count: alarms_config.max_alarms_group,
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
    tx_sender: TxRequestSender<Blocking>,
    result_receiver: CommitResultReceiver,
    context: TaskContext,
    tick_time: Duration,
    poll_time: Duration,
) -> Infallible {
    let result: Result<ChannelClosed, FatalError> = task_inner(
        node_client,
        tx_sender,
        result_receiver,
        context,
        tick_time,
        poll_time,
    )
    .await;

    match result {
        Ok(ChannelClosed {
            contract_type,
            contract_address,
        }) => {
            let contract_address: &str = contract_address.as_ref();

            loop {
                error!(
                    %contract_type,
                    %contract_address,
                    "Communication channel has been closed!"
                );

                sleep(tick_time).await;
            }
        },
        Err(FatalError {
            contract_type,
            contract_address,
            tx_hash: hash,
            tx_result: response,
        }) => {
            let contract_address: &str = contract_address.as_ref();

            loop {
                error!(
                    %contract_type,
                    %contract_address,
                    code = response.code.value(),
                    log = response.raw_log,
                    data = ?response.data,
                    %hash,
                    "Task encountered expected error!"
                );

                sleep(tick_time).await;
            }
        },
    }
}

struct ChannelClosed {
    contract_type: &'static str,
    contract_address: Arc<str>,
}

struct FatalError {
    contract_type: &'static str,
    contract_address: Arc<str>,
    tx_hash: TxHash,
    tx_result: TxResponse,
}

async fn task_inner(
    node_client: NodeClient,
    tx_sender: TxRequestSender<Blocking>,
    mut result_receiver: CommitResultReceiver,
    context: TaskContext,
    tick_time: Duration,
    poll_time: Duration,
) -> Result<ChannelClosed, FatalError> {
    let mut fallback_gas_limit: NonZeroU64 = context.hard_gas_limit;

    'runner_loop: loop {
        let should_send: bool = query::wasm_smart(
            &mut node_client.wasm_query_client(),
            context.contract_address.to_string(),
            QueryMsg::ALARMS_STATUS.to_vec(),
        )
        .await
        .map_or(true, |response: StatusResponse| {
            response.remaining_for_dispatch()
        });

        if should_send {
            'generator_loop: loop {
                if matches!(
                    send_tx(&tx_sender, &context, fallback_gas_limit),
                    Err(ChannelClosedError {})
                ) {
                    break 'runner_loop Ok(ChannelClosed {
                        contract_type: context.contract_type,
                        contract_address: context.contract_address,
                    });
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
                    },
                    Err(ChannelClosedError {}) => {
                        break 'runner_loop Ok(ChannelClosed {
                            contract_type: context.contract_type,
                            contract_address: context.contract_address,
                        });
                    },
                };

                let Some(response): Option<TxResponse> =
                    broadcast::poll_delivered_tx(
                        &node_client,
                        tick_time,
                        poll_time,
                        tx_hash.clone(),
                    )
                    .await
                else {
                    warn!("Transaction not found or couldn't be reported back in the given specified period.");

                    continue 'generator_loop;
                };

                match handle_response(
                    &context,
                    &mut fallback_gas_limit,
                    tx_hash,
                    response,
                ) {
                    HandleResponseResult::ContinueTxLooping => {
                        continue 'generator_loop;
                    },
                    HandleResponseResult::BreakTxLoop => {
                        break 'generator_loop;
                    },
                    HandleResponseResult::Fatal {
                        tx_hash,
                        tx_result: response,
                    } => {
                        break 'runner_loop Err(FatalError {
                            contract_type: context.contract_type,
                            contract_address: context.contract_address,
                            tx_hash,
                            tx_result: response,
                        });
                    },
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
        tx_result: TxResponse,
    },
}

fn handle_response(
    context: &TaskContext,
    fallback_gas_limit: &mut NonZeroU64,
    tx_hash: TxHash,
    tx_result: TxResponse,
) -> HandleResponseResult {
    let maybe_dispatched_count: Option<u32> = log::tx_response(
        context.contract_type,
        &context.contract_address,
        &tx_hash,
        &tx_result,
    )
    .map(|dispatch_response| dispatch_response.dispatched_alarms());

    let dispatched_count: u32 = match extract_dispatched_count(
        fallback_gas_limit,
        tx_hash,
        tx_result,
        maybe_dispatched_count,
    ) {
        Ok(dispatched_count) => dispatched_count,
        Err(ExtractDispatchedCountError::OutOfGas) => {
            return HandleResponseResult::ContinueTxLooping;
        },
        Err(ExtractDispatchedCountError::Fatal {
            tx_hash,
            tx_result: response,
        }) => {
            return HandleResponseResult::Fatal {
                tx_hash,
                tx_result: response,
            };
        },
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
    tx_sender: &TxRequestSender<Blocking>,
    context: &TaskContext,
    fallback_gas_limit: NonZeroU64,
) -> Result<(), ChannelClosedError> {
    let channel_closed: bool = tx_sender
        .send(TxRequest::<Blocking>::new(
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
        tx_result: TxResponse,
    },
}

fn extract_dispatched_count(
    fallback_gas_limit: &mut NonZeroU64,
    tx_hash: TxHash,
    tx_result: TxResponse,
    maybe_dispatched_count: Option<u32>,
) -> Result<u32, ExtractDispatchedCountError> {
    if tx_result.code.is_err() {
        if tx_result.code.value() == 11 {
            warn!("Transaction failed. Probable error is out of gas. Retrying transaction.");

            if let Some(gas_used) = NonZeroU64::new(tx_result.gas_used) {
                *fallback_gas_limit = gas_used.max(*fallback_gas_limit);
            }

            return Err(ExtractDispatchedCountError::OutOfGas);
        }
    } else {
        *fallback_gas_limit = NonZeroU64::new({
            let (mut n, overflow): (u64, bool) =
                fallback_gas_limit.get().overflowing_add(tx_result.gas_used);

            n >>= 1;

            if overflow {
                n |= 1 << (u64::BITS - 1);
            }

            n
        })
        .unwrap_or_else(|| unreachable!());

        if let Some(dispatched_count) = maybe_dispatched_count {
            return Ok(dispatched_count);
        }
    }

    Err(ExtractDispatchedCountError::Fatal { tx_hash, tx_result })
}

async fn receive_back_tx_hash(
    result_receiver: &mut CommitResultReceiver,
    contract_address: &Arc<str>,
    contract_type: &str,
) -> Result<Option<TxHash>, ChannelClosedError> {
    if let Some(result) = result_receiver.recv().await {
        match result {
            Ok(hash) => return Ok(Some(hash)),
            Err(CommitError {
                r#type,
                tx_response,
            }) => {
                error!(
                    contract_type = contract_type,
                    address = contract_address.as_ref(),
                    code = tx_response.code.value(),
                    log = tx_response.raw_log,
                    info = tx_response.info,
                    "Failed to commit transaction! Error type: {}",
                    match r#type {
                        CommitErrorType::InvalidAccountSequence =>
                            "Invalid account sequence",
                        CommitErrorType::Unknown => "Unknown",
                    },
                );
            },
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
