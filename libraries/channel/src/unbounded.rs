use std::{convert::Infallible, marker::PhantomData};

use tokio::sync::mpsc::{
    self,
    error::{SendError, TryRecvError},
};

use super::Closed;

pub enum Generic {}

impl super::Generic for Generic {
    type Channel<T>
        = Channel<T>
    where
        T: Send;
}

pub struct Channel<T>(PhantomData<T>, Infallible);

impl<T> super::Channel for Channel<T>
where
    T: Send,
{
    type Value = T;

    type Sender = mpsc::UnboundedSender<T>;

    type Receiver = mpsc::UnboundedReceiver<T>;

    #[inline]
    fn new() -> (Self::Sender, Self::Receiver) {
        mpsc::unbounded_channel()
    }
}

pub type Sender<T> = <Channel<T> as super::Channel>::Sender;

impl<T> super::Sender for mpsc::UnboundedSender<T>
where
    T: Send,
    Channel<T>: super::Channel<Sender = mpsc::UnboundedSender<T>>,
{
    type Value = T;

    async fn send(&self, value: Self::Value) -> Result<(), Closed> {
        Sender::send(self, value).map_err(|SendError(_)| Closed {})
    }
}

pub type Receiver<T> = <Channel<T> as super::Channel>::Receiver;

impl<T> super::Receiver for mpsc::UnboundedReceiver<T>
where
    T: Send,
    Channel<T>: super::Channel<Receiver = mpsc::UnboundedReceiver<T>>,
{
    type Value = T;

    async fn recv(&mut self) -> Result<Self::Value, Closed> {
        Receiver::recv(self).await.ok_or(Closed {})
    }

    fn try_recv(&mut self) -> Result<Option<Self::Value>, Closed> {
        match Receiver::try_recv(self) {
            Ok(value) => Ok(Some(value)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Closed {}),
        }
    }
}
