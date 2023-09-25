use std::path::Path;

use cosmrs::{crypto::secp256k1::SigningKey, proto::cosmos::auth::v1beta1::BaseAccount};
use serde::de::DeserializeOwned;
use tracing::info;

use crate::{
    account, client::Client, config, interact::query_account_data, signer::Signer,
    signing_key::signing_key,
};

use self::error::Result;

pub mod error;

#[non_exhaustive]
pub struct RpcSetup<C>
where
    C: AsRef<config::Node>,
{
    pub signer: Signer,
    pub config: C,
    pub nolus_node: Client,
}

pub async fn prepare_rpc<C, P>(config_path: P, key_derivation_path: &str) -> Result<RpcSetup<C>>
where
    C: DeserializeOwned + AsRef<config::Node>,
    P: AsRef<Path>,
{
    let signing_key: SigningKey = signing_key(key_derivation_path, "").await?;

    info!("Successfully derived private key.");

    let config: C = config::read::<C, P>(config_path).await?;

    info!("Successfully read configuration file.");

    let nolus_node: Client = Client::from_config(config.as_ref()).await?;

    info!("Fetching account data from network...");

    let address: String = account::id(config.as_ref(), &signing_key)?.to_string();

    let account_data: BaseAccount = query_account_data(&nolus_node, &address).await?;

    info!("Successfully fetched account data from network.");

    Ok(RpcSetup {
        signer: Signer::new(
            address,
            signing_key,
            config.as_ref().chain_id().clone(),
            account_data,
        ),
        config,
        nolus_node,
    })
}
