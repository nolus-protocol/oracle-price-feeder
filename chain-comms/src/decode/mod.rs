use base64::{
    alphabet::URL_SAFE,
    engine::{GeneralPurpose, GeneralPurposeConfig},
    Engine,
};

// TODO use this version when errors in wasmd are corrected
//  use cosmrs::cosmwasm::MsgExecuteContractResponse;
//  use cosmrs::tx::Msg as _;
use cosmrs::{proto::prost::Message, tendermint::abci::types::ExecTxResult, Any};

pub mod error;

pub fn exec_tx_data(tx: &ExecTxResult) -> Result<Vec<u8>, error::Error> {
    let data: Vec<u8> = Engine::decode(
        &GeneralPurpose::new(&URL_SAFE, GeneralPurposeConfig::new()),
        &tx.data,
    )?;

    // TODO use this version when errors in wasmd are corrected
    //
    // NOTE: outer Vec<u8> protobuf layer may not be needed after it's corrected
    // <Vec<u8> as Message>::decode(
    //     MsgExecuteContractResponse::from_any(&<Any as Message>::decode(
    //         <Vec<u8> as Message>::decode(data.as_slice())?.as_slice(),
    //     )?)?
    //     .data
    //     .as_slice(),
    // )
    // .map_err(Into::into)

    <Vec<u8> as Message>::decode(
        <Any as Message>::decode(<Vec<u8> as Message>::decode(data.as_slice())?.as_slice())?
            .value
            .as_slice(),
    )
    .map_err(Into::into)
}
