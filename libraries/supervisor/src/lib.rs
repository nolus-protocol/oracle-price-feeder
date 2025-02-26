use std::{
    future::{poll_fn, Future},
    pin::pin,
    pin::Pin,
    task::Poll,
};

use anyhow::Result;
use tokio::{io, signal::ctrl_c, task::JoinError};

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

    let mut terminate_signal = pin!(ctrl_c());

    let terminate_signal_sent = {
        let mut polled = false;

        poll_fn(|ctx| {
            if polled {
                Poll::Ready(Ok(false))
            } else {
                polled = true;

                terminate_signal
                    .as_mut()
                    .poll(ctx)
                    .map(|result| result.map(|()| true))
            }
        })
        .await?
    };

    if terminate_signal_sent {
        return Ok(state);
    }

    loop {
        state = match join_or_receive_or_terminate(
            &mut tasks,
            &mut rx,
            terminate_signal.as_mut(),
        )
        .await
        {
            JoinOrReceiveOrTerminate::Received(action_result) => {
                action_handler(&mut tasks, state, action_result).await?
            },
            JoinOrReceiveOrTerminate::ReceiverClosed => {
                break;
            },
            JoinOrReceiveOrTerminate::Joined(id, result) => {
                let Err(()) = log_errors(result) else {
                    continue;
                };

                on_error_exit(&mut tasks, state, id).await?
            },
            JoinOrReceiveOrTerminate::JoinSetEmpty => {
                return Ok(state);
            },
            JoinOrReceiveOrTerminate::Shutdown(result) => {
                return result.map(|()| state);
            },
        };
    }

    drop(rx);

    drop(action_handler);

    loop {
        state = match join_or_terminate(&mut tasks, terminate_signal.as_mut())
            .await
        {
            JoinOrTerminate::Joined(id, result) => {
                let Err(()) = log_errors(result) else {
                    continue;
                };

                on_error_exit(&mut tasks, state, id).await?
            },
            JoinOrTerminate::JoinSetEmpty => {
                break Ok(state);
            },
            JoinOrTerminate::Shutdown(result) => {
                break result.map(|()| state);
            },
        };
    }
}

fn log_errors(result: Result<Result<()>, JoinError>) -> Result<(), ()> {
    result
        .map_err(|error| {
            tracing::error!(?error, "Task joined with an error!");
        })
        .and_then(|result| {
            result.map_err(|error| {
                tracing::error!(?error, "Task joined with an error!");
            })
        })
}

async fn join_or_receive_or_terminate<Id, Receiver, TerminateSignal>(
    tasks: &mut TaskSet<Id, Result<()>>,
    rx: &mut Receiver,
    mut terminate_signal: Pin<&mut TerminateSignal>,
) -> JoinOrReceiveOrTerminate<Receiver::Value, Id>
where
    Id: Unpin,
    Receiver: channel::Receiver,
    TerminateSignal: Future<Output = Result<(), io::Error>>,
{
    let mut join_next_task = pin!(tasks.join_next());

    let mut receive_action = pin!(rx.recv());

    poll_fn({
        move |ctx| {
            if let Poll::Ready(receive_result) =
                receive_action.as_mut().poll(ctx)
            {
                Poll::Ready(match receive_result {
                    Ok(received) => {
                        JoinOrReceiveOrTerminate::Received(received)
                    },
                    Err(Closed {}) => JoinOrReceiveOrTerminate::ReceiverClosed,
                })
            } else if let Poll::Ready(join_result) =
                join_next_task.as_mut().poll(ctx)
            {
                Poll::Ready(join_result.map_or(
                    const { JoinOrReceiveOrTerminate::JoinSetEmpty },
                    |(id, result)| JoinOrReceiveOrTerminate::Joined(id, result),
                ))
            } else {
                terminate_signal
                    .as_mut()
                    .poll(ctx)
                    .map_err(Into::into)
                    .map(JoinOrReceiveOrTerminate::Shutdown)
            }
        }
    })
    .await
}

async fn join_or_terminate<Id, TerminateSignal>(
    tasks: &mut TaskSet<Id, Result<()>>,
    mut terminate_signal: Pin<&mut TerminateSignal>,
) -> JoinOrTerminate<Id>
where
    Id: Unpin,
    TerminateSignal: Future<Output = Result<(), io::Error>>,
{
    let mut join_next_task = pin!(tasks.join_next());

    poll_fn({
        move |ctx| {
            if let Poll::Ready(join_result) = join_next_task.as_mut().poll(ctx)
            {
                Poll::Ready(join_result.map_or(
                    const { JoinOrTerminate::JoinSetEmpty },
                    |(id, result)| JoinOrTerminate::Joined(id, result),
                ))
            } else {
                terminate_signal
                    .as_mut()
                    .poll(ctx)
                    .map_err(Into::into)
                    .map(JoinOrTerminate::Shutdown)
            }
        }
    })
    .await
}

enum JoinOrReceiveOrTerminate<Value, Id> {
    Received(Value),
    ReceiverClosed,
    Joined(Id, Result<Result<()>, JoinError>),
    JoinSetEmpty,
    Shutdown(Result<()>),
}

enum JoinOrTerminate<Id> {
    Joined(Id, Result<Result<()>, JoinError>),
    JoinSetEmpty,
    Shutdown(Result<()>),
}
