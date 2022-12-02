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
