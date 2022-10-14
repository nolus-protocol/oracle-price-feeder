use std::str::FromStr;

use cosmrs::{
    cosmwasm::MsgExecuteContract,
    rpc::{self, endpoint::broadcast::tx_commit::Response},
    tx::Msg,
    AccountId,
};
use serde::{Deserialize, Serialize};

pub use client::CosmosClient;
pub use tx::TxBuilder;
pub use wallet::Wallet;

use crate::{configuration::Oracle, provider::Price};

use self::error::CosmosError;

pub mod client;
pub mod error;
pub mod json;
mod tx;
mod wallet;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    SupportedDenomPairs {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    FeedPrices { prices: Vec<Price> },
}

pub async fn broadcast_tx(
    client: &CosmosClient,
    wallet: &Wallet,
    config: &Oracle,
    data: String,
) -> Result<Response, CosmosError> {
    let sender_account_id = wallet.get_sender_account_id(config.prefix.as_str())?;
    let account_data = client
        .get_account_data(sender_account_id.to_string().as_str())
        .await?;

    let exec_msg = MsgExecuteContract {
        sender: sender_account_id,
        contract: AccountId::from_str(&config.contract_addrs.to_string())?,
        msg: data.as_bytes().to_vec(),
        funds: vec![],
    }
    .to_any()?;

    let tx_raw = TxBuilder::new(config.chain_id.to_owned())?
        .memo("Test memo")
        .account_info(account_data.sequence, account_data.account_number)
        .timeout_height(0)
        .fee(
            config.fee_denom.to_owned(),
            config.funds_amount,
            config.gas_limit,
        )?
        .add_message(exec_msg)
        .sign(wallet)?;

    let rpc_client =
        rpc::HttpClient::new(format!("{}:{}", config.host_url, config.rpc_port).as_str())?;

    Ok(tx_raw.broadcast_commit(&rpc_client).await?)
}
