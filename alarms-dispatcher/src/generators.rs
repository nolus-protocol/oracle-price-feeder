use std::{collections::BTreeMap, num::NonZeroU32, num::NonZeroU64, sync::Arc, time::Duration};

use tokio::{sync::mpsc::unbounded_channel, task::JoinSet, time::sleep};
use tracing::{error, info, warn};

use chain_comms::{
    client::Client as NodeClient,
    interact::query,
    reexport::cosmrs::{
        proto::cosmwasm::wasm::v1::MsgExecuteContract, tendermint::Hash as TxHash,
        tx::MessageExt as _, Any as ProtobufAny,
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
    signer_address: String,
    tx_sender: &broadcast::TxRequestSender,
    market_price_oracle_contracts: Vec<Contract>,
    time_alarms_contracts: Vec<Contract>,
    tick_time: Duration,
    poll_time: Duration,
) -> Result<broadcast::SpawnGeneratorsResult, DispatchAlarmsError> {
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
                signer_address.clone(),
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
        .map(|()| broadcast::SpawnGeneratorsResult::new(tx_generators_set, tx_result_senders))
}

struct SpawnTxGeneratorContext {
    tx_sender: broadcast::TxRequestSender,
    monotonic_id: usize,
    contract: Contract,
    contract_type: &'static str,
}

fn spawn_single(
    signer_address: String,
    node_client: &NodeClient,
    tx_generators_set: &mut JoinSet<()>,
    tx_result_senders: &mut BTreeMap<usize, broadcast::CommitResultSender>,
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

    let (tx_result_sender, tx_result_receiver): (
        broadcast::CommitResultSender,
        broadcast::CommitResultReceiver,
    ) = unbounded_channel();

    tx_result_senders.insert(monotonic_id, tx_result_sender);

    tx_generators_set.spawn(task(
        node_client.clone(),
        tx_sender,
        tx_result_receiver,
        GeneratorTaskContext {
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

struct GeneratorTaskContext {
    monotonic_id: usize,
    contract_address: Arc<str>,
    max_alarms_count: NonZeroU32,
    messages: Box<[ProtobufAny]>,
    contract_type: &'static str,
    hard_gas_limit: NonZeroU64,
}

async fn task(
    node_client: NodeClient,
    tx_sender: broadcast::TxRequestSender,
    mut result_receiver: broadcast::CommitResultReceiver,
    GeneratorTaskContext {
        monotonic_id,
        contract_address,
        max_alarms_count,
        messages,
        contract_type,
        hard_gas_limit,
    }: GeneratorTaskContext,
    tick_time: Duration,
    poll_time: Duration,
) {
    let last_hash;

    let last_response;

    'channel_closed: {
        'runner_loop: loop {
            let should_send = node_client
                .with_grpc(|rpc| {
                    query::wasm(rpc, contract_address.to_string(), QueryMsg::ALARMS_STATUS)
                })
                .await
                .map_or(true, |response: StatusResponse| {
                    response.remaining_for_dispatch()
                });

            if should_send {
                'generator_loop: loop {
                    let channel_closed = tx_sender
                        .send(broadcast::TxRequest::new(
                            monotonic_id,
                            messages.to_vec(),
                            hard_gas_limit,
                        ))
                        .is_err();

                    if channel_closed {
                        warn!(
                            contract_type = contract_type,
                            contract_address = contract_address.as_ref(),
                            "Channel closed. Exiting task.",
                        );

                        break;
                    }

                    let tx_hash = match receive_back_tx_hash(
                        &mut result_receiver,
                        &contract_address,
                        contract_type,
                    )
                    .await
                    {
                        Ok(Some(hash)) => hash,
                        Ok(None) => continue 'generator_loop,
                        Err(ChannelClosedError) => break 'channel_closed,
                    };

                    let Some(response) =
                        broadcast::poll_delivered_tx(&node_client, tick_time, poll_time, tx_hash)
                            .await
                    else {
                        warn!("Transaction not found or couldn't be reported back in the given specified period.");

                        continue 'generator_loop;
                    };

                    let maybe_dispatched_count =
                        log::tx_response(contract_type, &contract_address, &tx_hash, &response)
                            .map(|dispatch_response| dispatch_response.dispatched_alarms());

                    let dispatched_count = 'dispatched_count: {
                        if response.code.is_err() {
                            if response.code.value() == 11 {
                                warn!("Transaction failed. Probable error is out of gas. Retrying transaction.");

                                continue 'generator_loop;
                            }
                        } else if let Some(dispatched_count) = maybe_dispatched_count {
                            break 'dispatched_count dispatched_count;
                        }

                        last_hash = tx_hash;

                        last_response = response;

                        break 'runner_loop;
                    };

                    if dispatched_count < max_alarms_count.get() {
                        info!("No alarms should be left for the time being.");

                        break 'generator_loop;
                    }
                }
            }

            sleep(tick_time).await;
        }

        drop(node_client);

        drop(messages);

        drop(tx_sender);

        drop(result_receiver);

        loop {
            error!(
                contract_type = contract_type,
                address = contract_address.as_ref(),
                code = last_response.code.value(),
                log = last_response.log,
                data = ?last_response.data,
                hash = %last_hash,
                "Task encountered expected error!"
            );

            sleep(tick_time).await;
        }
    }
}

struct ChannelClosedError;

async fn receive_back_tx_hash(
    result_receiver: &mut broadcast::CommitResultReceiver,
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
                        broadcast::CommitErrorType::InvalidAccountSequence => "Invalid account sequence",
                        broadcast::CommitErrorType::Unknown => "Unknown",
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
