use base64::{
    alphabet::URL_SAFE,
    engine::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};
use cosmrs::{cosmwasm::MsgExecuteContractResponse, proto::prost::Message, tx::Msg as _, Any};

use crate::interact::get_tx_response::Response as TxResponse;

use self::error::Error;

pub mod error;

#[derive(Message)]
struct Package {
    #[prost(bytes, tag = "2")]
    data: Vec<u8>,
}

pub fn tx_response_data(tx: &TxResponse) -> Result<Vec<u8>, Error> {
    Engine::decode(
        &GeneralPurpose::new(&URL_SAFE, GeneralPurposeConfig::new()),
        tx.data.as_bytes(),
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
        tx_response_data(&TxResponse {
            code: Default::default(),
            block_height: 0,
            data: b"EjQKLC9jb3Ntd2FzbS53YXNtLnYxLk1zZ0V4ZWN1dGVDb250cmFjdFJlc3BvbnNlEgQKAjE2"
                .to_vec()
                .into(),
            raw_log: Default::default(),
            info: Default::default(),
            gas_wanted: 0,
            gas_used: 0,
        })
        .unwrap()
        .as_slice(),
        b"16"
    );
}
