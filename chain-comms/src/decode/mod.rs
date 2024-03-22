use cosmrs::{
    cosmwasm::MsgExecuteContractResponse,
    proto::{prost::Message, Any as ProtobufAny},
    tx::Msg as _,
};
use data_encoding::HEXUPPER;

use crate::interact::get_tx_response::Response as TxResponse;

use self::error::Error;

pub mod error;

#[derive(Message)]
struct Package {
    #[prost(bytes, tag = "2")]
    data: Vec<u8>,
}

pub fn tx_response_data(tx: &TxResponse) -> Result<Vec<u8>, Error> {
    HEXUPPER
        .decode(tx.data.as_bytes())
        .map_err(Error::Decode)
        .and_then(|data| {
            Message::decode(data.as_slice()).map_err(Error::Deserialize)
        })
        .and_then(|Package { data }| {
            Message::decode(data.as_slice()).map_err(Error::Deserialize)
        })
        .and_then(|any: ProtobufAny| {
            MsgExecuteContractResponse::from_any(&any)
                .map_err(Error::InvalidResponseType)
        })
        .map(|MsgExecuteContractResponse { data }| data)
}

#[cfg(test)]
#[test]
fn test() {
    assert_eq!(
        String::from_utf8(
            tx_response_data(&TxResponse {
                code: Default::default(),
                block_height: 0,
                data: "12340A2C2F636F736D7761736D2E7761736D2E76312E4D736745786563757465436F6E7472616374526573706F6E736512040A023332".into(),
                raw_log: Default::default(),
                info: Box::default(),
                gas_wanted: 0,
                gas_used: 0,
            })
                .unwrap()
        )
            .unwrap(),
        "32"
    );
}
