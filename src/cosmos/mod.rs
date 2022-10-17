use std::str::FromStr;

use cosmrs::{
    cosmwasm::MsgExecuteContract,
    rpc::{self, endpoint::broadcast::tx_commit::Response},
    tx::Msg,
    AccountId,
};
use serde::{Deserialize, Serialize};

use crate::{configuration::Oracle, provider::Price};

use self::error::CosmosError;
pub use self::{client::CosmosClient, tx::TxBuilder, wallet::Wallet};

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
    let sender_account_id = wallet.get_sender_account_id(&config.prefix)?;
    let account_data = client.get_account_data(sender_account_id.as_ref()).await?;

    let exec_msg = MsgExecuteContract {
        sender: sender_account_id,
        contract: AccountId::from_str(&config.contract_addrs)?,
        msg: data.into_bytes(),
        funds: vec![],
    }
    .to_any()?;

    let tx_raw = TxBuilder::new(config.chain_id.clone())?
        .memo(String::from("Test memo"))
        .account_info(account_data.sequence, account_data.account_number)
        .timeout_height(0)
        .fee(&config.fee_denom, config.funds_amount, config.gas_limit)?
        .add_message(exec_msg)
        .sign(wallet)?;

    let rpc_client =
        rpc::HttpClient::new(format!("{}:{}", config.host_url, config.rpc_port).as_str())?;

    tx_raw
        .broadcast_commit(&rpc_client)
        .await
        .map_err(Into::into)
}
