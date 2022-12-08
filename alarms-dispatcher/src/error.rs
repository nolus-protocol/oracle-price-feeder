use std::fmt::{Debug, Display, Formatter};

use cosmrs::ErrorReport;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("Couldn't construct a valid URI with given configuration! Context: {0}")]
    InvalidUri(#[from] tonic::codegen::http::uri::InvalidUri),
    #[error("gRPC transport error has occurred! Context: {0}")]
    GrpcTransport(#[from] tonic::transport::Error),
    #[error("gRPC request returned this status: {0}")]
    GrpcRequest(#[from] tonic::Status),
    #[error("Tendermint error has occurred! Context: {0}")]
    Tendermint(#[from] cosmrs::rpc::Error),
    #[error("Couldn't encode Protobuf message! Context: {0}")]
    EncodeProtobuf(#[from] prost::EncodeError),
    #[error("Couldn't decode Protobuf message! Context: {0}")]
    DecodeProtobuf(#[from] prost::DecodeError),
    #[error("Deriving account ID failed!")]
    AccountIdDerivationFailed,
    #[error("Account not found!")]
    AccountNotFound,
    #[error("Signing data failed! Context: {0}")]
    Signing(ErrorReport),
    #[error("Broadcasting transaction failed! Context: {0}")]
    BroadcastTx(ErrorReport),
}

#[derive(Debug, ThisError)]
pub struct ContextError<E>
where
    E: Debug + Display,
{
    error: E,
    context: Vec<String>,
}

impl<E> ContextError<E>
where
    E: Debug + Display,
{
    pub fn map<NewE>(self) -> ContextError<NewE>
    where
        E: Into<NewE>,
        NewE: Debug + Display,
    {
        ContextError {
            error: self.error.into(),
            context: self.context,
        }
    }

    pub fn into_inner(self) -> E {
        self.error
    }
}

impl<E> Display for ContextError<E>
where
    E: Debug + Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}\nCaller stack context:", self.error))?;

        self.context
            .iter()
            .try_for_each(move |context| f.write_fmt(format_args!("\n\t=> {context}")))
    }
}

pub trait WithOriginContext
where
    Self: Debug + Display,
{
    type ContextHolder: std::error::Error;

    fn with_origin_context(self, context: &str) -> Self::ContextHolder;
}

impl<E> WithOriginContext for E
where
    E: Debug + Display,
{
    type ContextHolder = ContextError<E>;

    fn with_origin_context(self, context: &str) -> ContextError<Self> {
        ContextError {
            error: self,
            context: vec![context.into()],
        }
    }
}

pub trait WithCallerContext {
    fn with_caller_context(self, context: &str) -> Self;
}

impl<E> WithCallerContext for ContextError<E>
where
    E: Debug + Display,
{
    fn with_caller_context(mut self, context: &str) -> Self {
        self.context.push(context.into());

        self
    }
}

impl<T, E> WithCallerContext for Result<T, E>
where
    E: WithCallerContext,
{
    fn with_caller_context(self, context: &str) -> Self {
        self.map_err(move |error| error.with_caller_context(context))
    }
}
