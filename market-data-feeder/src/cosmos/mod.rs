use std::str::FromStr;

use cosmrs::{
    cosmwasm::MsgExecuteContract,
    proto::cosmos::auth::v1beta1::BaseAccount,
    rpc::{self},
    tx::{Msg, Raw},
    AccountId,
};
use serde::{Deserialize, Serialize};

use crate::{configuration::Oracle, provider::Price};

use self::error::Cosmos as CosmosError;
pub use self::{
    client::Client,
    tx::{Builder as TxBuilder, TxResponse},
    wallet::Wallet,
};

pub mod client;
pub mod error;
pub mod json;
mod tx;
mod wallet;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    SupportedCurrencyPairs {},
}

pub type PoolId = u64;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SwapLeg {
    pub from: String,
    pub to: SwapTarget,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SwapTarget {
    pub pool_id: PoolId,
    pub target: String,
}

pub type SupportedCurrencyPairsResponse = Vec<SwapLeg>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    FeedPrices { prices: Box<[Price]> },
    DispatchAlarms { max_count: u32 },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AlarmsResponse {
    RemainingForDispatch {},
    NextAlarm { unix_time: u64 },
}

#[inline]
pub fn get_sender_account_id(wallet: &Wallet, config: &Oracle) -> Result<AccountId, CosmosError> {
    wallet
        .get_sender_account_id(&config.prefix)
        .map_err(Into::into)
}

#[inline]
pub async fn get_account_data(
    client: &Client,
    account_id: &AccountId,
) -> Result<BaseAccount, CosmosError> {
    client
        .get_account_data(account_id.as_ref())
        .await
        .map_err(Into::into)
}

pub fn construct_tx(
    sender_account_id: &AccountId,
    account_data: &BaseAccount,
    wallet: &Wallet,
    config: &Oracle,
    data: String,
) -> Result<Raw, CosmosError> {
    let exec_msg = MsgExecuteContract {
        sender: sender_account_id.clone(),
        contract: AccountId::from_str(&config.contract_addrs)?,
        msg: data.into_bytes(),
        funds: vec![],
    }
    .to_any()?;

    TxBuilder::new(&config.chain_id)?
        .memo(String::from("Test memo"))
        .account_info(account_data.sequence, account_data.account_number)
        .timeout_height(0)
        .fee(&config.fee_denom, config.funds_amount, config.gas_limit)?
        .add_message(exec_msg)
        .sign(wallet)
        .map_err(Into::into)
}

pub fn construct_rpc_client(config: &Oracle) -> Result<rpc::HttpClient, CosmosError> {
    rpc::HttpClient::new(format!("{}:{}", config.host_url, config.rpc_port).as_str())
        .map_err(Into::into)
}
