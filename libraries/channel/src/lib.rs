use thiserror::Error;

pub mod bounded;
pub mod unbounded;

pub trait Generic {
    type Channel<T>: Channel<Value = T>
    where
        T: Send;
}

pub trait Channel {
    type Value: Send;

    type Sender: Sender<Value = Self::Value>;

    type Receiver: Receiver<Value = Self::Value>;

    fn new() -> (Self::Sender, Self::Receiver);
}

pub trait Sender {
    type Value: Send;

    fn send(
        &self,
        value: Self::Value,
    ) -> impl Future<Output = Result<(), Closed>> + Send + '_;
}

pub trait Receiver {
    type Value: Send;

    fn recv(
        &mut self,
    ) -> impl Future<Output = Result<Self::Value, Closed>> + Send + '_;

    fn try_recv(&mut self) -> Result<Option<Self::Value>, Closed>;
}

#[derive(Debug, Error)]
#[error("Channel closed!")]
pub struct Closed;
