use cosmrs::{
    proto::{cosmos::base::v1beta1::Coin, cosmwasm::wasm::v1::MsgExecuteContract},
    tx::{Body, Fee, MessageExt, Raw as RawTx},
    Any,
};

use crate::{error::Error, log_error, signer::Signer};

struct Msg {
    message: Vec<u8>,
    funds: Vec<Coin>,
}

pub struct ContractMsgs {
    address: String,
    messages: Vec<Msg>,
}

impl ContractMsgs {
    pub const fn new(contract_address: String) -> Self {
        Self {
            address: contract_address,
            messages: Vec::new(),
        }
    }

    pub fn add_message(mut self, message: Vec<u8>, funds: Vec<Coin>) -> Self {
        self.messages.push(Msg { message, funds });

        self
    }

    pub fn commit(
        self,
        signer: &Signer,
        fee: Fee,
        memo: Option<&str>,
        timeout: Option<u32>,
    ) -> Result<RawTx, Error> {
        signer.sign(
            Body::new(
                {
                    let buf = Vec::with_capacity(self.messages.len());

                    log_error!(
                        self.messages
                            .into_iter()
                            .map(|msg| {
                                MsgExecuteContract {
                                    sender: signer.signer_address().into(),
                                    contract: self.address.clone(),
                                    msg: msg.message,
                                    funds: msg.funds,
                                }
                                .to_any()
                            })
                            .try_fold(buf, |mut acc, msg| -> Result<Vec<Any>, Error> {
                                acc.push(msg?);

                                Ok(acc)
                            }),
                        "Failed serializing transaction messages!"
                    )?
                },
                memo.unwrap_or_default(),
                timeout.unwrap_or_default(),
            ),
            fee,
        )
    }
}
