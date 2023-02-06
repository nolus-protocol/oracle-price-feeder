use std::path::Path;

use serde::de::DeserializeOwned;
use tracing::info;

use crate::{
    account::{account_data, account_id},
    client::Client,
    config::{read_config, Node},
    signer::Signer,
    signing_key::signing_key,
};

use self::error::Result;

pub mod error;

#[non_exhaustive]
pub struct RpcSetup<C>
where
    C: AsRef<Node>,
{
    pub signer: Signer,
    pub config: C,
    pub client: Client,
}

pub async fn prepare_rpc<C, P>(config_path: P, key_derivation_path: &str) -> Result<RpcSetup<C>>
where
    C: DeserializeOwned + AsRef<Node>,
    P: AsRef<Path>,
{
    let signing_key = signing_key(key_derivation_path, "").await?;

    info!("Successfully derived private key.");

    let config: C = read_config::<C, P>(config_path).await?;

    info!("Successfully read configuration file.");

    let client = Client::new(config.as_ref()).await?;

    info!("Fetching account data from network...");

    let account_id = account_id(config.as_ref(), &signing_key)?;

    let account_data = account_data(&client, account_id.clone()).await?;

    info!("Successfully fetched account data from network.");

    Ok(RpcSetup {
        signer: Signer::new(
            account_id.to_string(),
            signing_key,
            config.as_ref().chain_id().clone(),
            account_data,
        ),
        config,
        client,
    })
}