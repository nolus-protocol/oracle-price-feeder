use std::{
    future::poll_fn,
    pin::{Pin, pin},
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

    loop {
        state = match await_with_action_receiver(
            &mut tasks,
            &mut rx,
            terminate_signal.as_mut(),
        )
        .await
        {
            AwaitWithActionResult::Received(action_result) => {
                action_handler(&mut tasks, state, action_result).await?
            },
            AwaitWithActionResult::ReceiverClosed => {
                break;
            },
            AwaitWithActionResult::Joined(id, result) => {
                let Err(()) = log_errors(result) else {
                    continue;
                };

                on_error_exit(&mut tasks, state, id).await?
            },
            AwaitWithActionResult::JoinSetEmpty => {
                return Ok(state);
            },
            AwaitWithActionResult::Shutdown(result) => {
                return result.map(|()| state);
            },
        };
    }

    drop(rx);

    drop(action_handler);

    loop {
        state = match await_without_action_receiver(
            &mut tasks,
            terminate_signal.as_mut(),
        )
        .await
        {
            AwaitWithoutActionResult::Joined(id, result) => {
                let Err(()) = log_errors(result) else {
                    continue;
                };

                on_error_exit(&mut tasks, state, id).await?
            },
            AwaitWithoutActionResult::JoinSetEmpty => {
                break Ok(state);
            },
            AwaitWithoutActionResult::Shutdown(result) => {
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

async fn await_with_action_receiver<Id, Receiver, TerminateSignal>(
    tasks: &mut TaskSet<Id, Result<()>>,
    rx: &mut Receiver,
    mut terminate_signal: Pin<&mut TerminateSignal>,
) -> AwaitWithActionResult<Receiver::Value, Id>
where
    Id: Unpin,
    Receiver: channel::Receiver,
    TerminateSignal: Future<Output = Result<(), io::Error>>,
{
    let mut join_next_task = pin!(tasks.join_next());

    let mut receive_action = pin!(rx.recv());

    poll_fn(move |ctx| {
        if let Poll::Ready(receive_result) = receive_action.as_mut().poll(ctx) {
            Poll::Ready(match receive_result {
                Ok(received) => AwaitWithActionResult::Received(received),
                Err(Closed {}) => AwaitWithActionResult::ReceiverClosed,
            })
        } else if let Poll::Ready(join_result) =
            join_next_task.as_mut().poll(ctx)
        {
            Poll::Ready(match join_result {
                Some((id, result)) => AwaitWithActionResult::Joined(id, result),
                None => AwaitWithActionResult::JoinSetEmpty,
            })
        } else {
            match terminate_signal.as_mut().poll(ctx) {
                Poll::Ready(Ok(())) => const {
                    Poll::Ready(AwaitWithActionResult::Shutdown(Ok(())))
                },
                Poll::Ready(Err(error)) => Poll::Ready(
                    AwaitWithActionResult::Shutdown(Err(error.into())),
                ),
                Poll::Pending => const { Poll::Pending },
            }
        }
    })
    .await
}

async fn await_without_action_receiver<Id, TerminateSignal>(
    tasks: &mut TaskSet<Id, Result<()>>,
    mut terminate_signal: Pin<&mut TerminateSignal>,
) -> AwaitWithoutActionResult<Id>
where
    Id: Unpin,
    TerminateSignal: Future<Output = Result<(), io::Error>>,
{
    let mut join_next_task = pin!(tasks.join_next());

    poll_fn(move |ctx| {
        if let Poll::Ready(join_result) = join_next_task.as_mut().poll(ctx) {
            Poll::Ready(join_result.map_or(
                const { AwaitWithoutActionResult::JoinSetEmpty },
                |(id, result)| AwaitWithoutActionResult::Joined(id, result),
            ))
        } else {
            terminate_signal
                .as_mut()
                .poll(ctx)
                .map_err(Into::into)
                .map(AwaitWithoutActionResult::Shutdown)
        }
    })
    .await
}

enum AwaitWithActionResult<Value, Id> {
    Received(Value),
    ReceiverClosed,
    Joined(Id, Result<Result<()>, JoinError>),
    JoinSetEmpty,
    Shutdown(Result<()>),
}

enum AwaitWithoutActionResult<Id> {
    Joined(Id, Result<Result<()>, JoinError>),
    JoinSetEmpty,
    Shutdown(Result<()>),
}
