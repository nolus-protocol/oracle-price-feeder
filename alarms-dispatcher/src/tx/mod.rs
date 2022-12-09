use cosmrs::{
    proto::{cosmos::base::v1beta1::Coin, cosmwasm::wasm::v1::MsgExecuteContract},
    tx::{Body, Fee, MessageExt, Raw as RawTx},
    Any,
};

use crate::signer::Signer;

use self::error::Result;

pub mod error;

struct Msg {
    message: Vec<u8>,
    funds: Vec<Coin>,
}

pub struct ContractTx {
    contract: String,
    messages: Vec<Msg>,
}

impl ContractTx {
    pub const fn new(contract: String) -> Self {
        Self {
            contract,
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
    ) -> Result<RawTx> {
        signer
            .sign(
                Body::new(
                    {
                        let buf = Vec::with_capacity(self.messages.len());

                        self.messages
                            .into_iter()
                            .map(|msg| {
                                MsgExecuteContract {
                                    sender: signer.signer_address().into(),
                                    contract: self.contract.clone(),
                                    msg: msg.message,
                                    funds: msg.funds,
                                }
                                .to_any()
                            })
                            .try_fold(buf, |mut acc, msg| -> Result<Vec<Any>> {
                                acc.push(msg?);

                                Ok(acc)
                            })?
                    },
                    memo.unwrap_or_default(),
                    timeout.unwrap_or_default(),
                ),
                fee,
            )
            .map_err(Into::into)
    }
}
