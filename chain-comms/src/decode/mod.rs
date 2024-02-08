use base64::{
    alphabet::URL_SAFE,
    engine::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use cosmrs::{
    cosmwasm::MsgExecuteContractResponse, proto::prost::Message,
    tendermint::abci::types::ExecTxResult, tx::Msg as _, Any,
};

use self::error::Error;

pub mod error;

#[derive(Message)]
struct Package {
    #[prost(bytes, tag = "2")]
    data: Vec<u8>,
}

pub fn exec_tx_data(tx: &ExecTxResult) -> Result<Vec<u8>, Error> {
    Engine::decode(
        &GeneralPurpose::new(&URL_SAFE, GeneralPurposeConfig::new()),
        &tx.data,
    )
    .map_err(Error::DecodeBase64)
    .and_then(|data| {
        <Package as cosmrs::proto::traits::Message>::decode(data.as_slice())
            .map_err(Error::DeserializeData)
    })
    .and_then(|Package { data }| {
        <Any as Message>::decode(data.as_slice()).map_err(Error::DeserializeData)
    })
    .and_then(|any| MsgExecuteContractResponse::from_any(&any).map_err(Error::InvalidResponseType))
    .map(|MsgExecuteContractResponse { data }| data)
}

#[cfg(test)]
#[test]
fn test() {
    assert_eq!(
        exec_tx_data(&ExecTxResult {
            code: Default::default(),
            data: b"EjQKLC9jb3Ntd2FzbS53YXNtLnYxLk1zZ0V4ZWN1dGVDb250cmFjdFJlc3BvbnNlEgQKAjE2"
                .to_vec()
                .into(),
            log: String::new(),
            info: String::new(),
            gas_wanted: 0,
            gas_used: 0,
            events: Vec::new(),
            codespace: String::new(),
        })
        .unwrap()
        .as_slice(),
        b"16"
    );
}
