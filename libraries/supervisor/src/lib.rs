use std::{
    future::{poll_fn, Future},
    pin::pin,
    task::Poll,
};

use anyhow::Result;
use tokio::task::JoinError;

use channel::{Channel, Closed};
use task_set::TaskSet;

pub async fn supervisor<
    Id,
    State,
    ActionsChannel,
    InitFn,
    ActionHandlerFn,
    OnErrorExitFn,
>(
    init: InitFn,
    mut action_handler: ActionHandlerFn,
    mut on_error_exit: OnErrorExitFn,
) -> Result<State>
where
    Id: Unpin,
    ActionsChannel: Channel,
    InitFn: for<'r> AsyncFnOnce(
        &'r mut TaskSet<Id, Result<()>>,
        ActionsChannel::Sender,
    ) -> Result<State>,
    ActionHandlerFn: for<'r> AsyncFnMut(
        &'r mut TaskSet<Id, Result<()>>,
        State,
        ActionsChannel::Value,
    ) -> Result<State>,
    OnErrorExitFn: for<'r> AsyncFnMut(
        &'r mut TaskSet<Id, Result<()>>,
        State,
        Id,
    ) -> Result<State>,
{
    let mut tasks = TaskSet::new();

    let (tx, mut rx) = ActionsChannel::new();

    let mut state = init(&mut tasks, tx).await?;

    loop {
        state = match join_or_receive(&mut tasks, &mut rx).await {
            JoinOrReceive::Received(action_result) => {
                action_handler(&mut tasks, state, action_result).await?
            },
            JoinOrReceive::ReceiverClosed => {
                break;
            },
            JoinOrReceive::Joined(id, result) => {
                let Err(()) = log_errors(result) else {
                    continue;
                };

                on_error_exit(&mut tasks, state, id).await?
            },
            JoinOrReceive::JoinSetEmpty => {
                return Ok(state);
            },
        };
    }

    drop(rx);

    drop(action_handler);

    while let Some((id, result)) = tasks.join_next().await {
        let Err(()) = log_errors(result) else {
            continue;
        };

        state = on_error_exit(&mut tasks, state, id).await?;
    }

    Ok(state)
}

fn log_errors(result: Result<Result<()>, JoinError>) -> Result<(), ()> {
    result.map_err(drop).and_then(|result| {
        result.map_err(|error| {
            tracing::error!(?error, "Task joined with an error!");
        })
    })
}

async fn join_or_receive<Id, Receiver>(
    tasks: &mut TaskSet<Id, Result<()>>,
    rx: &mut Receiver,
) -> JoinOrReceive<Receiver::Value, Id>
where
    Id: Unpin,
    Receiver: channel::Receiver,
{
    let mut join_next_task = pin!(tasks.join_next());

    let mut receive_action = pin!(rx.recv());

    poll_fn({
        move |ctx| {
            if let Poll::Ready(receive_result) =
                receive_action.as_mut().poll(ctx)
            {
                Poll::Ready(match receive_result {
                    Ok(received) => JoinOrReceive::Received(received),
                    Err(Closed {}) => JoinOrReceive::ReceiverClosed,
                })
            } else {
                join_next_task.as_mut().poll(ctx).map(|join_result| {
                    join_result.map_or(
                        const { JoinOrReceive::JoinSetEmpty },
                        |(id, result)| JoinOrReceive::Joined(id, result),
                    )
                })
            }
        }
    })
    .await
}

enum JoinOrReceive<Value, Id> {
    Received(Value),
    ReceiverClosed,
    Joined(Id, Result<Result<()>, JoinError>),
    JoinSetEmpty,
}
