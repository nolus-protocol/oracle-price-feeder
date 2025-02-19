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

    type Sender = mpsc::Sender<T>;

    type Receiver = mpsc::Receiver<T>;

    #[inline]
    fn new() -> (Self::Sender, Self::Receiver) {
        mpsc::channel(16)
    }
}

pub type Sender<T> = <Channel<T> as super::Channel>::Sender;

impl<T> super::Sender for mpsc::Sender<T>
where
    T: Send,
    Channel<T>: super::Channel<Sender = mpsc::Sender<T>>,
{
    type Value = T;

    async fn send(&self, value: Self::Value) -> Result<(), Closed> {
        Sender::send(self, value)
            .await
            .map_err(|SendError(_)| Closed {})
    }
}

pub type Receiver<T> = <Channel<T> as super::Channel>::Receiver;

impl<T> super::Receiver for mpsc::Receiver<T>
where
    T: Send,
    Channel<T>: super::Channel<Receiver = mpsc::Receiver<T>>,
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
